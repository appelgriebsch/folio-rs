use std::io::Read;

use dom_query::{Document, NodeRef};
use image::ImageFormat;
use reqwest::blocking::Client;
use url::Url;

pub const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;
const SHORT_DATA_BASE64: usize = 140;

#[derive(Debug, Clone)]
pub struct EmbeddedImage {
    pub href: String,
    pub bytes: Vec<u8>,
    pub media_type: String,
}

#[derive(Debug, Default)]
pub struct ImageWarnings {
    pub urls: Vec<String>,
}

impl ImageWarnings {
    pub fn push(&mut self, url: &str) {
        self.urls.push(warn_url(url));
    }

    pub fn emit(&self) {
        let show = self.urls.len().min(10);
        for url in &self.urls[..show] {
            eprintln!("warning: omitted image {url}");
        }
        let rest = self.urls.len().saturating_sub(10);
        if rest > 0 {
            eprintln!("warning: omitted {rest} more images");
        }
    }
}

fn warn_url(url: &str) -> String {
    if url.starts_with("data:") && url.chars().count() > 120 {
        let end = url
            .char_indices()
            .nth(120)
            .map(|(i, _)| i)
            .unwrap_or(url.len());
        format!("{}…", &url[..end])
    } else {
        url.to_string()
    }
}

/// Copy a noscript image onto a previous placeholder `img`, and copy TeX out of `math`.
pub fn recover_specials(doc: &Document) {
    let noscripts: Vec<NodeRef<'_>> = doc.select("noscript").nodes().to_vec();
    for noscript in noscripts {
        let Some(prev) = noscript.prev_element_sibling() else {
            continue;
        };
        if !name_is(&prev, "img") || !is_placeholder_img(&prev) {
            continue;
        }
        let Some(attrs) = noscript_img(&noscript) else {
            continue;
        };
        if let Some(src) = &attrs.src {
            prev.set_attr("src", src);
        } else {
            prev.remove_attr("src");
        }
        if let Some(srcset) = &attrs.srcset {
            prev.set_attr("srcset", srcset);
        } else {
            prev.remove_attr("srcset");
        }
        if let Some(data_src) = &attrs.data_src {
            prev.set_attr("data-src", data_src);
        } else {
            prev.remove_attr("data-src");
        }
        if let Some(alt) = &attrs.alt {
            prev.set_attr("alt", alt);
        }
        prev.remove_attr("width");
        prev.remove_attr("height");
        noscript.remove_from_parent();
    }

    let maths: Vec<NodeRef<'_>> = doc.select("math").nodes().to_vec();
    for math in maths {
        let Some(tex) = tex_annotation(&math) else {
            continue;
        };
        math.before_html(format!("<p>{}</p>", crate::xhtml::escape_text(&tex)));
        math.remove_from_parent();
    }
}

struct ImgAttrs {
    src: Option<String>,
    srcset: Option<String>,
    data_src: Option<String>,
    alt: Option<String>,
}

fn noscript_img(noscript: &NodeRef<'_>) -> Option<ImgAttrs> {
    let elements = noscript.element_children();
    if elements.len() == 1 && name_is(&elements[0], "img") {
        return Some(attrs_of(&elements[0]));
    }
    if !elements.is_empty() {
        return None;
    }
    let text = noscript.text();
    let frag = Document::fragment(text);
    let top = frag.tree.root().element_children();
    if top.len() == 1 && name_is(&top[0], "img") {
        Some(attrs_of(&top[0]))
    } else {
        None
    }
}

fn attrs_of(img: &NodeRef<'_>) -> ImgAttrs {
    ImgAttrs {
        src: attr(img, "src"),
        srcset: attr(img, "srcset"),
        data_src: attr(img, "data-src"),
        alt: attr(img, "alt"),
    }
}

fn tex_annotation(math: &NodeRef<'_>) -> Option<String> {
    for node in math.descendants() {
        if !name_is(&node, "annotation") {
            continue;
        }
        let encoding = attr(&node, "encoding").unwrap_or_default();
        if encoding.to_ascii_lowercase().contains("tex") {
            let text = node.text();
            if !text.trim().is_empty() {
                return Some(text.trim().to_string());
            }
        }
    }
    None
}

/// Flatten each kept image to one absolute `src` plus ordered fallbacks.
pub fn flatten_images(html: &str, base: &Url) -> String {
    let doc = Document::fragment(html);
    let pictures: Vec<NodeRef<'_>> = doc.select("picture").nodes().to_vec();
    for picture in pictures {
        flatten_picture(&picture, base);
    }
    let imgs: Vec<NodeRef<'_>> = doc.select("img").nodes().to_vec();
    for img in imgs {
        if inside_picture(&img) || attr(&img, "data-folio-src").is_some() {
            continue;
        }
        apply_candidates(&img, &candidates_for(&img, base));
    }
    doc.tree.root().inner_html().to_string()
}

fn flatten_picture(picture: &NodeRef<'_>, base: &Url) {
    let children = picture.element_children();
    let img = children.iter().find(|node| name_is(node, "img")).copied();
    let sources: Vec<NodeRef<'_>> = children
        .iter()
        .filter(|node| name_is(node, "source"))
        .copied()
        .collect();
    let mut chosen = Vec::new();
    if let Some(img) = img {
        if has_real_src_or_srcset(&img, base) {
            chosen = candidates_for(&img, base);
        }
    }
    if chosen.is_empty() {
        for source in &sources {
            let kind = attr(source, "type").unwrap_or_default();
            if !is_raster_mime(&kind) {
                continue;
            }
            let found = candidates_for(source, base);
            if !found.is_empty() {
                chosen = found;
                break;
            }
        }
    }
    if chosen.is_empty() {
        if let Some(img) = img {
            chosen = candidates_for(&img, base);
        }
    }
    for source in &sources {
        source.remove_from_parent();
    }
    if chosen.is_empty() {
        return;
    }
    let img = picture
        .element_children()
        .into_iter()
        .find(|node| name_is(node, "img"))
        .unwrap_or_else(|| {
            let created = picture.tree.new_element("img");
            picture.append_child(&created);
            created
        });
    apply_candidates(&img, &chosen);
}

fn apply_candidates(img: &NodeRef<'_>, urls: &[Url]) {
    let placeholder_box = dims_are_one(img);
    img.remove_attr("srcset");
    img.remove_attr("data-src");
    if placeholder_box {
        img.remove_attr("width");
        img.remove_attr("height");
    }
    if urls.is_empty() {
        img.remove_attr("src");
        img.remove_attr("data-folio-src");
        return;
    }
    img.set_attr("src", urls[0].as_str());
    let joined = urls.iter().map(Url::as_str).collect::<Vec<_>>().join(" ");
    img.set_attr("data-folio-src", &joined);
}

fn has_real_src_or_srcset(node: &NodeRef<'_>, base: &Url) -> bool {
    if usable_srcset(node).is_some() {
        return true;
    }
    attr(node, "src")
        .and_then(|raw| resolve(base, &raw))
        .is_some_and(|url| src_is_real(&url, dims_are_one(node)))
}

fn candidates_for(node: &NodeRef<'_>, base: &Url) -> Vec<Url> {
    let dims_one = dims_are_one(node);
    if let Some(srcset) = usable_srcset(node) {
        let mut urls = Vec::new();
        for candidate in srcset {
            if let Some(url) = resolve(base, &candidate.url) {
                if !urls.iter().any(|have: &Url| have == &url) {
                    urls.push(url);
                }
            }
        }
        if !urls.is_empty() {
            return urls;
        }
    }
    if let Some(raw) = attr(node, "src") {
        if let Some(url) = resolve(base, &raw) {
            if src_is_real(&url, dims_one) {
                return vec![url];
            }
        }
    }
    if let Some(raw) = attr(node, "data-src") {
        if let Some(url) = resolve(base, &raw) {
            if data_src_usable(&url) {
                return vec![url];
            }
        }
    }
    Vec::new()
}

fn usable_srcset(node: &NodeRef<'_>) -> Option<Vec<SrcCandidate>> {
    let raw = attr(node, "srcset")?;
    let parsed = parse_srcset(&raw);
    if parsed.is_empty() {
        return None;
    }
    let has_width = parsed.iter().any(|item| item.width.is_some());
    let has_density = parsed.iter().any(|item| item.density.is_some());
    if has_width && has_density {
        return None;
    }
    let mut parsed = parsed;
    if has_width {
        parsed.sort_by(|a, b| b.width.unwrap_or(0).cmp(&a.width.unwrap_or(0)));
    } else {
        parsed.sort_by(|a, b| {
            b.density
                .unwrap_or(1.0)
                .partial_cmp(&a.density.unwrap_or(1.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    Some(parsed)
}

#[derive(Debug, Clone, PartialEq)]
struct SrcCandidate {
    url: String,
    width: Option<u32>,
    density: Option<f32>,
}

fn strip_suffix_ignore_ascii(value: &str, suffix: char) -> Option<&str> {
    let mut chars = value.chars();
    let last = chars.next_back()?;
    if last.eq_ignore_ascii_case(&suffix) {
        Some(chars.as_str())
    } else {
        None
    }
}

fn parse_srcset(input: &str) -> Vec<SrcCandidate> {
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b',') {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let url = input[start..i].to_string();
        let mut width = None;
        let mut density = None;
        loop {
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= bytes.len() || bytes[i] == b',' {
                break;
            }
            let desc_start = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b',' {
                i += 1;
            }
            let desc = &input[desc_start..i];
            if let Some(value) = strip_suffix_ignore_ascii(desc, 'w') {
                if let Ok(number) = value.parse::<u32>() {
                    width = Some(number);
                }
            } else if let Some(value) = strip_suffix_ignore_ascii(desc, 'x') {
                if let Ok(number) = value.parse::<f32>() {
                    density = Some(number);
                }
            }
        }
        if !url.is_empty() {
            out.push(SrcCandidate {
                url,
                width,
                density,
            });
        }
    }
    out
}

fn resolve(base: &Url, raw: &str) -> Option<Url> {
    let raw = raw.trim();
    if raw.is_empty() || raw.starts_with('#') {
        return None;
    }
    if raw.starts_with("data:") {
        return Url::parse(raw).ok();
    }
    let joined = base.join(raw).ok()?;
    match joined.scheme() {
        "http" | "https" | "data" => Some(joined),
        _ => None,
    }
}

fn src_is_real(url: &Url, dims_one: bool) -> bool {
    if dims_one {
        return false;
    }
    match url.scheme() {
        "http" | "https" => true,
        "data" => !data_url_is_placeholder(url),
        _ => false,
    }
}

fn data_src_usable(url: &Url) -> bool {
    match url.scheme() {
        "http" | "https" => true,
        "data" => !data_url_is_placeholder(url),
        _ => false,
    }
}

fn is_placeholder_img(img: &NodeRef<'_>) -> bool {
    if dims_are_one(img) {
        return true;
    }
    match attr(img, "src") {
        Some(src) => match Url::parse(src.trim()) {
            Ok(url) => match url.scheme() {
                "data" => data_url_is_placeholder(&url),
                "http" | "https" => false,
                _ => true,
            },
            Err(_) => false,
        },
        None => true,
    }
}

fn dims_are_one(node: &NodeRef<'_>) -> bool {
    attr(node, "width").is_some_and(|v| dim_is_one(&v))
        && attr(node, "height").is_some_and(|v| dim_is_one(&v))
}

fn dim_is_one(value: &str) -> bool {
    let value = value.trim().trim_end_matches("px").trim();
    value == "1"
}

fn data_url_is_placeholder(url: &Url) -> bool {
    let Some((mime, bytes)) = decode_data_url(url.as_str()) else {
        return true;
    };
    if mime.as_deref().unwrap_or("").contains("svg") {
        return false;
    }
    let payload = url
        .as_str()
        .split_once(',')
        .map(|(_, data)| data)
        .unwrap_or("");
    let compact: String = payload.chars().filter(|ch| !ch.is_whitespace()).collect();
    if compact.len() <= SHORT_DATA_BASE64 {
        return true;
    }
    raster_is_one_pixel(&bytes)
}

fn is_raster_mime(value: &str) -> bool {
    let value = value
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    matches!(
        value.as_str(),
        "image/jpeg" | "image/jpg" | "image/pjpeg" | "image/png" | "image/gif"
    )
}

fn inside_picture(node: &NodeRef<'_>) -> bool {
    node.ancestors(None)
        .iter()
        .any(|ancestor| name_is(ancestor, "picture"))
}

/// Fetch one http(s) or data image for the EPUB cover. The href is `images/cover.{ext}`.
pub fn download_cover(client: &Client, url: &str) -> Option<EmbeddedImage> {
    let mut image = fetch_image(client, url, 1).ok()?;
    let extension = image
        .href
        .rsplit('.')
        .next()
        .filter(|ext| !ext.is_empty())
        .unwrap_or("img");
    image.href = format!("images/cover.{extension}");
    Some(image)
}

pub fn download_images(
    html: &str,
    client: &Client,
    warnings: &mut ImageWarnings,
) -> (String, Vec<EmbeddedImage>) {
    let doc = Document::fragment(html);
    let imgs: Vec<NodeRef<'_>> = doc.select("img").nodes().to_vec();
    let mut embedded = Vec::new();
    let mut cache: Vec<(String, Option<String>)> = Vec::new();
    for img in imgs {
        let list = attr(&img, "data-folio-src")
            .or_else(|| attr(&img, "src"))
            .unwrap_or_default();
        let urls: Vec<String> = list.split_whitespace().map(str::to_string).collect();
        img.remove_attr("data-folio-src");
        if urls.is_empty() {
            img.remove_from_parent();
            continue;
        }
        match first_embedded(client, &urls, &mut cache, &mut embedded, warnings) {
            Some(href) => img.set_attr("src", &href),
            None => img.remove_from_parent(),
        }
    }
    (doc.tree.root().inner_html().to_string(), embedded)
}

fn first_embedded(
    client: &Client,
    urls: &[String],
    cache: &mut Vec<(String, Option<String>)>,
    embedded: &mut Vec<EmbeddedImage>,
    warnings: &mut ImageWarnings,
) -> Option<String> {
    for url in urls {
        if let Some((_, cached)) = cache.iter().find(|(key, _)| key == url) {
            if cached.is_some() {
                return cached.clone();
            }
            continue;
        }
        match fetch_image(client, url, embedded.len() + 1) {
            Ok(asset) => {
                let href = asset.href.clone();
                embedded.push(asset);
                cache.push((url.clone(), Some(href.clone())));
                return Some(href);
            }
            Err(()) => cache.push((url.clone(), None)),
        }
    }
    warnings.push(&urls[0]);
    None
}

fn fetch_image(client: &Client, url: &str, index: usize) -> Result<EmbeddedImage, ()> {
    let (mime, bytes) = if url.starts_with("data:") {
        decode_data_url(url).ok_or(())?
    } else {
        let parsed = Url::parse(url).map_err(|_| ())?;
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return Err(());
        }
        let response = client.get(parsed).send().map_err(|_| ())?;
        if !response.status().is_success() {
            return Err(());
        }
        if response
            .content_length()
            .is_some_and(|len| len > MAX_IMAGE_BYTES as u64)
        {
            return Err(());
        }
        let mime = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.split(';').next().unwrap_or("").trim().to_string());
        let bytes = read_capped(response, MAX_IMAGE_BYTES)?;
        (mime, bytes)
    };
    let accepted = accept_image(&bytes, mime.as_deref()).ok_or(())?;
    Ok(EmbeddedImage {
        href: format!("images/img-{index}.{}", accepted.extension),
        bytes: accepted.bytes,
        media_type: accepted.media_type,
    })
}

struct Accepted {
    bytes: Vec<u8>,
    media_type: String,
    extension: &'static str,
}

fn accept_image(bytes: &[u8], content_type: Option<&str>) -> Option<Accepted> {
    let hinted_svg = content_type
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .eq_ignore_ascii_case("image/svg+xml");
    if hinted_svg || looks_like_svg(bytes) {
        if !svg_is_safe(bytes) {
            return None;
        }
        return Some(Accepted {
            bytes: bytes.to_vec(),
            media_type: "image/svg+xml".to_string(),
            extension: "svg",
        });
    }
    match image::guess_format(bytes) {
        Ok(ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::Gif) => {}
        _ => return None,
    }
    match decode_limited(bytes) {
        Ok(image) => {
            if image.width() == 1 || image.height() == 1 {
                return None;
            }
            let (media_type, extension) = match image::guess_format(bytes).ok()? {
                ImageFormat::Jpeg => ("image/jpeg", "jpg"),
                ImageFormat::Png => ("image/png", "png"),
                ImageFormat::Gif => ("image/gif", "gif"),
                _ => return None,
            };
            Some(Accepted {
                bytes: bytes.to_vec(),
                media_type: media_type.to_string(),
                extension,
            })
        }
        Err(_) => None,
    }
}

fn raster_is_one_pixel(bytes: &[u8]) -> bool {
    decode_limited(bytes)
        .map(|image| image.width() == 1 || image.height() == 1)
        .unwrap_or(false)
}

const MAX_IMAGE_EDGE: u32 = 8000;
const MAX_IMAGE_ALLOC: u64 = 32 * 1024 * 1024;

fn decode_limited(bytes: &[u8]) -> image::ImageResult<image::DynamicImage> {
    decode_with_limits(bytes, MAX_IMAGE_EDGE, MAX_IMAGE_ALLOC)
}

fn decode_with_limits(
    bytes: &[u8],
    max_edge: u32,
    max_alloc: u64,
) -> image::ImageResult<image::DynamicImage> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(image::ImageError::IoError)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(max_edge);
    limits.max_image_height = Some(max_edge);
    limits.max_alloc = Some(max_alloc);
    reader.limits(limits);
    reader.decode()
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(512)];
    let text = String::from_utf8_lossy(head)
        .trim_start()
        .to_ascii_lowercase();
    text.starts_with("<svg")
        || text.starts_with("<?xml") && text.contains("<svg")
        || text.contains("<svg")
}

fn svg_is_safe(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let lower = text.to_ascii_lowercase();
    if !lower.contains("<svg") || lower.contains("javascript:") {
        return false;
    }
    svg_markup_is_safe(&lower)
}

fn svg_markup_is_safe(lower: &str) -> bool {
    let bytes = lower.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        if lower[i..].starts_with("<!--") {
            let Some(end) = lower[i + 4..].find("-->") else {
                return false;
            };
            i += 4 + end + 3;
            continue;
        }
        if lower[i..].starts_with("<![cdata[") {
            let Some(end) = lower[i + 9..].find("]]>") else {
                return false;
            };
            i += 9 + end + 3;
            continue;
        }
        if lower[i..].starts_with("<?") {
            let Some(end) = lower[i + 2..].find("?>") else {
                return false;
            };
            i += 2 + end + 2;
            continue;
        }
        if lower[i..].starts_with("<!") {
            return false;
        }
        let closing = lower[i..].starts_with("</");
        let name_at = if closing { i + 2 } else { i + 1 };
        let Some(name_end) = xml_name_end(bytes, name_at) else {
            return false;
        };
        if name_end == name_at || forbidden_svg_name(local_name(&lower[name_at..name_end])) {
            return false;
        }
        if closing {
            let Some(rel) = lower[name_end..].find('>') else {
                return false;
            };
            i = name_end + rel + 1;
            continue;
        }
        match scan_svg_attributes(lower, name_end) {
            Some(next) => i = next,
            None => return false,
        }
    }
    true
}

fn forbidden_svg_name(name: &str) -> bool {
    matches!(
        name,
        "script" | "foreignobject" | "iframe" | "embed" | "object"
    )
}

fn local_name(name: &str) -> &str {
    name.rsplit_once(':')
        .map(|(_, local)| local)
        .unwrap_or(name)
}

fn xml_name_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut index = start;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':') {
            index += 1;
        } else {
            break;
        }
    }
    Some(index)
}

fn scan_svg_attributes(lower: &str, mut index: usize) -> Option<usize> {
    let bytes = lower.as_bytes();
    loop {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() {
            return None;
        }
        if bytes[index] == b'>' {
            return Some(index + 1);
        }
        if bytes[index] == b'/' {
            index += 1;
            continue;
        }
        let attr_start = index;
        let attr_end = xml_name_end(bytes, attr_start)?;
        if attr_end == attr_start {
            return None;
        }
        let attr = local_name(&lower[attr_start..attr_end]);
        if attr.starts_with("on") && attr.len() > 2 {
            return None;
        }
        index = attr_end;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index < bytes.len() && bytes[index] == b'=' {
            index += 1;
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            if index >= bytes.len() {
                return None;
            }
            if bytes[index] == b'"' || bytes[index] == b'\'' {
                let quote = bytes[index];
                index += 1;
                let start = index;
                while index < bytes.len() && bytes[index] != quote {
                    index += 1;
                }
                if index >= bytes.len() {
                    return None;
                }
                if lower[start..index].contains("javascript:") {
                    return None;
                }
                index += 1;
            } else {
                let start = index;
                while index < bytes.len()
                    && !bytes[index].is_ascii_whitespace()
                    && bytes[index] != b'>'
                    && bytes[index] != b'/'
                {
                    index += 1;
                }
                if lower[start..index].contains("javascript:") {
                    return None;
                }
            }
        }
    }
}

fn decode_data_url(url: &str) -> Option<(Option<String>, Vec<u8>)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    let mut mime = None;
    let mut base64 = false;
    for (index, part) in meta.split(';').enumerate() {
        if index == 0 && !part.is_empty() && part != "base64" {
            mime = Some(part.to_ascii_lowercase());
        }
        if part.eq_ignore_ascii_case("base64") {
            base64 = true;
        }
    }
    let bytes = if base64 {
        decode_base64(data)?
    } else {
        percent_decode(data)
    };
    Some((mime, bytes))
}

fn decode_base64(input: &str) -> Option<Vec<u8>> {
    fn val(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' | b'-' => Some(62),
            b'/' | b'_' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::new();
    let mut buf = [0u8; 4];
    let mut n = 0;
    for byte in input.bytes() {
        if byte.is_ascii_whitespace() {
            continue;
        }
        if byte == b'=' {
            break;
        }
        buf[n] = val(byte)?;
        n += 1;
        if n == 4 {
            out.push((buf[0] << 2) | (buf[1] >> 4));
            out.push((buf[1] << 4) | (buf[2] >> 2));
            out.push((buf[2] << 6) | buf[3]);
            n = 0;
        }
    }
    if n == 1 {
        return None;
    }
    if n >= 2 {
        out.push((buf[0] << 2) | (buf[1] >> 4));
    }
    if n >= 3 {
        out.push((buf[1] << 4) | (buf[2] >> 2));
    }
    Some(out)
}

fn percent_decode(input: &str) -> Vec<u8> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(value) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                out.push(value);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    out
}

fn read_capped(mut response: reqwest::blocking::Response, max: usize) -> Result<Vec<u8>, ()> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let read = response.read(&mut chunk).map_err(|_| ())?;
        if read == 0 {
            return Ok(buf);
        }
        if buf.len() + read > max {
            return Err(());
        }
        buf.extend_from_slice(&chunk[..read]);
    }
}

fn name_is(node: &NodeRef<'_>, name: &str) -> bool {
    node.node_name()
        .is_some_and(|value| value.eq_ignore_ascii_case(name))
}

fn attr(node: &NodeRef<'_>, name: &str) -> Option<String> {
    node.attr(name)
        .map(|value| value.to_string())
        .filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::{accept_image, flatten_images, parse_srcset, recover_specials};
    use dom_query::Document;
    use image::ImageEncoder;

    fn base() -> Url {
        Url::parse("http://cdn.example/articles/").unwrap()
    }

    #[test]
    fn srcset_picks_largest_width_and_ignores_mixed() {
        let html = r#"<img src="small.jpg" srcset="a.jpg 100w, b.jpg 800w, c.jpg 400w">"#;
        let flat = flatten_images(html, &base());
        assert!(flat.contains("http://cdn.example/articles/b.jpg"), "{flat}");
        assert!(!flat.contains("srcset"), "{flat}");

        let mixed = r#"<img src="keep.jpg" srcset="a.jpg 100w, b.jpg 2x">"#;
        let flat = flatten_images(mixed, &base());
        assert!(
            flat.contains("http://cdn.example/articles/keep.jpg"),
            "{flat}"
        );
        assert!(!flat.contains("a.jpg"), "{flat}");
    }

    #[test]
    fn density_and_real_src_beats_data_src() {
        let html = r#"<img src="photo.jpg" data-src="other.jpg" srcset="one.jpg 1x, two.jpg 2x">"#;
        let flat = flatten_images(html, &base());
        assert!(flat.contains("two.jpg"), "{flat}");
        assert!(!flat.contains("other.jpg"), "{flat}");

        let real = r#"<img src="photo.jpg" data-src="other.jpg">"#;
        let flat = flatten_images(real, &base());
        assert!(flat.contains("photo.jpg"), "{flat}");
        assert!(!flat.contains("other.jpg"), "{flat}");
    }

    #[test]
    fn placeholder_yields_to_data_src_and_picture_source() {
        let html = r#"<img src="data:image/gif;base64,R0lGODlhAQABAAAAACw=" data-src="/real.png" width="1" height="1">"#;
        let flat = flatten_images(html, &base());
        assert!(flat.contains("http://cdn.example/real.png"), "{flat}");
        assert!(!flat.contains("width="), "{flat}");
        assert!(!flat.contains("height="), "{flat}");

        let picture = r#"
            <picture>
              <source type="image/webp" srcset="/no.webp">
              <source type="image/png" srcset="/from-source.png">
              <img src="data:image/gif;base64,R0lGODlhAQABAAAAACw=">
            </picture>
        "#;
        let flat = flatten_images(picture, &base());
        assert!(
            flat.contains("http://cdn.example/from-source.png"),
            "{flat}"
        );
        assert!(!flat.contains("no.webp"), "{flat}");
    }

    #[test]
    fn base_not_canonical() {
        let html = r#"<img src="pic.png">"#;
        let flat = flatten_images(html, &base());
        assert!(
            flat.contains("http://cdn.example/articles/pic.png"),
            "{flat}"
        );
    }

    #[test]
    fn recovers_noscript_image_and_tex() {
        let html = r#"
            <p><img src="data:image/gif;base64,R0lGODlhAQABAAAAACw=" width="1" height="1">
            <noscript><img src="http://cdn.example/real.png" alt="real"></noscript></p>
            <math><semantics><annotation encoding="application/x-tex">E = mc^2</annotation></semantics></math>
        "#;
        let doc = Document::from(html);
        recover_specials(&doc);
        let out = doc.tree.root().html().to_string();
        assert!(out.contains("http://cdn.example/real.png"), "{out}");
        assert!(out.contains("E = mc^2"), "{out}");
        assert!(!out.contains("<math"), "{out}");
        assert!(!out.contains("<noscript"), "{out}");
    }

    #[test]
    fn keeps_small_decoded_image_and_drops_tracker_and_unsafe_svg() {
        let png = tiny_png(2, 2);
        assert!(accept_image(&png, Some("image/png")).is_some());
        assert!(png.len() < 2048);

        let one = tiny_png(1, 1);
        assert!(accept_image(&one, Some("image/png")).is_none());

        assert!(accept_image(&[0, 1, 2, 3], None).is_none());
        assert!(accept_image(&vec![9u8; 3000], None).is_none());

        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="4" height="4"/></svg>"#;
        assert!(accept_image(svg, Some("image/svg+xml")).is_some());
        let bad = br#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#;
        assert!(accept_image(bad, Some("image/svg+xml")).is_none());
        let foreign = br#"<svg><foreignObject></foreignObject></svg>"#;
        assert!(accept_image(foreign, Some("image/svg+xml")).is_none());
        let prefixed =
            br#"<svg xmlns="http://www.w3.org/2000/svg"><x:script>alert(1)</x:script></svg>"#;
        assert!(accept_image(prefixed, Some("image/svg+xml")).is_none());
        let handler =
            br#"<svg xmlns="http://www.w3.org/2000/svg"><rect onclick="alert(1)"/></svg>"#;
        assert!(accept_image(handler, Some("image/svg+xml")).is_none());
        let doctype = br#"<svg><!DOCTYPE svg [<!ENTITY xxe SYSTEM "file:///etc/passwd">]></svg>"#;
        assert!(accept_image(doctype, Some("image/svg+xml")).is_none());
    }

    #[test]
    fn decode_limit_rejects_an_oversized_edge() {
        let png = tiny_png(2, 2);
        let err = super::decode_with_limits(&png, 1, super::MAX_IMAGE_ALLOC).unwrap_err();
        assert!(matches!(err, image::ImageError::Limits(_)), "{err}");
    }

    #[test]
    fn parses_srcset_descriptors() {
        let parsed = parse_srcset("a.jpg 100w, b.jpg 2x");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].width, Some(100));
        assert_eq!(parsed[1].density, Some(2.0));
    }

    fn tiny_png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        let image = image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x * 40) as u8, (y * 40) as u8, 20])
        });
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(
                image.as_raw(),
                width,
                height,
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();
        bytes
    }
}
