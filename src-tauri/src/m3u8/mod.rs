pub mod parser;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PlaylistKind {
    Master,
    Media,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariantInfo {
    pub url: String,
    pub bandwidth: Option<u64>,
    pub resolution: Option<String>,
    pub name: Option<String>,
    pub codecs: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyInfo {
    pub method: String,
    pub uri: Option<String>,
    pub iv: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentInfo {
    pub index: usize,
    pub url: String,
    pub duration: f64,
    pub key: Option<KeyInfo>,
    pub byte_range: Option<(u64, Option<u64>)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaPlaylist {
    pub url: String,
    pub target_duration: Option<f64>,
    pub media_sequence: u64,
    pub segments: Vec<SegmentInfo>,
    pub end_list: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeResult {
    pub kind: PlaylistKind,
    pub url: String,
    pub variants: Vec<VariantInfo>,
    pub media: Option<MediaPlaylist>,
}
