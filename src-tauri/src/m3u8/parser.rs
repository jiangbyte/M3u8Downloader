use crate::error::{AppError, AppResult};
use crate::m3u8::{
    AnalyzeResult, KeyInfo, MediaPlaylist, PlaylistKind, SegmentInfo, VariantInfo,
};
use url::Url;

fn resolve_url(base: &Url, reference: &str) -> AppResult<String> {
    Ok(base.join(reference.trim())?.to_string())
}

fn parse_attributes(s: &str) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    let mut rest = s.trim();
    while !rest.is_empty() {
        let Some(eq) = rest.find('=') else { break };
        let key = rest[..eq].trim().to_string();
        rest = &rest[eq + 1..];
        let (value, next) = if rest.starts_with('"') {
            let end = rest[1..]
                .find('"')
                .map(|i| i + 1)
                .unwrap_or(rest.len().saturating_sub(1));
            let value = rest[1..end].to_string();
            let after = if end + 1 < rest.len() {
                &rest[end + 1..]
            } else {
                ""
            };
            let after = after.trim_start_matches(',').trim_start();
            (value, after)
        } else {
            let end = rest.find(',').unwrap_or(rest.len());
            let value = rest[..end].trim().to_string();
            let after = if end < rest.len() {
                rest[end + 1..].trim_start()
            } else {
                ""
            };
            (value, after)
        };
        attrs.push((key, value));
        rest = next;
    }
    attrs
}

fn attr_map(s: &str) -> std::collections::HashMap<String, String> {
    parse_attributes(s).into_iter().collect()
}

pub fn parse_playlist(content: &str, playlist_url: &str) -> AppResult<AnalyzeResult> {
    let base = Url::parse(playlist_url)?;
    let lines: Vec<&str> = content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    if lines.first().map(|l| *l) != Some("#EXTM3U") {
        return Err(AppError::msg("Not a valid M3U8 playlist (missing #EXTM3U)"));
    }

    let is_master = lines.iter().any(|l| l.starts_with("#EXT-X-STREAM-INF"));
    if is_master {
        let mut variants = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];
            if line.starts_with("#EXT-X-STREAM-INF:") {
                let attrs = attr_map(&line["#EXT-X-STREAM-INF:".len()..]);
                i += 1;
                while i < lines.len() && lines[i].starts_with('#') {
                    i += 1;
                }
                if i >= lines.len() {
                    break;
                }
                let uri = resolve_url(&base, lines[i])?;
                let bandwidth = attrs
                    .get("BANDWIDTH")
                    .or_else(|| attrs.get("AVERAGE-BANDWIDTH"))
                    .and_then(|v| v.parse().ok());
                let resolution = attrs.get("RESOLUTION").cloned();
                let name = attrs.get("NAME").cloned();
                let codecs = attrs.get("CODECS").cloned();
                variants.push(VariantInfo {
                    url: uri,
                    bandwidth,
                    resolution,
                    name,
                    codecs,
                });
            }
            i += 1;
        }
        if variants.is_empty() {
            return Err(AppError::msg("Master playlist has no variants"));
        }
        return Ok(AnalyzeResult {
            kind: PlaylistKind::Master,
            url: playlist_url.to_string(),
            variants,
            media: None,
        });
    }

    let mut target_duration = None;
    let mut media_sequence = 0u64;
    let mut end_list = false;
    let mut current_key: Option<KeyInfo> = None;
    let mut pending_duration: Option<f64> = None;
    let mut pending_byte_range: Option<(u64, Option<u64>)> = None;
    let mut segments = Vec::new();
    let mut index = 0usize;

    for line in lines.iter().skip(1) {
        if line.starts_with("#EXT-X-TARGETDURATION:") {
            target_duration = line
                .split_once(':')
                .and_then(|(_, v)| v.parse().ok());
        } else if line.starts_with("#EXT-X-MEDIA-SEQUENCE:") {
            media_sequence = line
                .split_once(':')
                .and_then(|(_, v)| v.parse().ok())
                .unwrap_or(0);
        } else if *line == "#EXT-X-ENDLIST" {
            end_list = true;
        } else if line.starts_with("#EXT-X-KEY:") {
            let attrs = attr_map(&line["#EXT-X-KEY:".len()..]);
            let method = attrs
                .get("METHOD")
                .cloned()
                .unwrap_or_else(|| "NONE".into());
            if method == "NONE" {
                current_key = None;
            } else {
                let uri = match attrs.get("URI") {
                    Some(u) => Some(resolve_url(&base, u)?),
                    None => None,
                };
                current_key = Some(KeyInfo {
                    method,
                    uri,
                    iv: attrs.get("IV").cloned(),
                });
            }
        } else if line.starts_with("#EXTINF:") {
            let raw = &line["#EXTINF:".len()..];
            let dur = raw
                .split(',')
                .next()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0);
            pending_duration = Some(dur);
        } else if line.starts_with("#EXT-X-BYTERANGE:") {
            let raw = &line["#EXT-X-BYTERANGE:".len()..];
            let mut parts = raw.split('@');
            let length: u64 = parts
                .next()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let start = parts.next().and_then(|v| v.parse().ok());
            pending_byte_range = Some((length, start));
        } else if !line.starts_with('#') {
            let duration = pending_duration.take().unwrap_or(0.0);
            let url = resolve_url(&base, line)?;
            segments.push(SegmentInfo {
                index,
                url,
                duration,
                key: current_key.clone(),
                byte_range: pending_byte_range.take(),
            });
            index += 1;
        }
    }

    if segments.is_empty() {
        return Err(AppError::msg("Media playlist has no segments"));
    }

    Ok(AnalyzeResult {
        kind: PlaylistKind::Media,
        url: playlist_url.to_string(),
        variants: vec![],
        media: Some(MediaPlaylist {
            url: playlist_url.to_string(),
            target_duration,
            media_sequence,
            segments,
            end_list,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_media() {
        let content = r#"#EXTM3U
#EXT-X-TARGETDURATION:10
#EXT-X-MEDIA-SEQUENCE:0
#EXTINF:9.0,
seg0.ts
#EXTINF:9.0,
seg1.ts
#EXT-X-ENDLIST
"#;
        let r = parse_playlist(content, "https://example.com/a/index.m3u8").unwrap();
        assert_eq!(r.kind, PlaylistKind::Media);
        let media = r.media.unwrap();
        assert_eq!(media.segments.len(), 2);
        assert!(media.segments[0].url.ends_with("/a/seg0.ts"));
    }

    #[test]
    fn parse_master() {
        let content = r#"#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360
low.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1280x720
high.m3u8
"#;
        let r = parse_playlist(content, "https://example.com/master.m3u8").unwrap();
        assert_eq!(r.kind, PlaylistKind::Master);
        assert_eq!(r.variants.len(), 2);
    }
}
