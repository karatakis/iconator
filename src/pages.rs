use std::path::Path;
use std::sync::OnceLock;

use axum::extract::Query;
use axum::response::Html;
use base64::Engine as _;
use maud::{DOCTYPE, Markup, html};
use serde::Deserialize;

use crate::api::{Kind, lookup};
use crate::error::ApiError;

#[derive(Debug, Default, Deserialize)]
pub struct IndexParams {
    path: Option<String>,
    kind: Option<String>,
}

/// GET / — lookup form on top, view-only grid of the full icon set below.
pub async fn index(Query(params): Query<IndexParams>) -> Result<Html<Markup>, ApiError> {
    let body = html! {
        (DOCTYPE)
        html {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "iconator" }
                style { (CSS) }
            }
            body {
                h1 { "iconator" }
                p { "Resolve file and folder paths to icon IDs. Icons are served at " code { "/icons/{id}.svg" } "." }
                section {
                    h2 { "Lookup" }
                    form method="get" action="/" {
                        input type="text" name="path" placeholder="e.g. src/main.rs"
                            value=[params.path.as_deref().filter(|p| !p.is_empty())];
                        select name="kind" {
                            @for (value, label, selected) in [
                                ("file", "file", params.kind.as_deref() != Some("folder")),
                                ("folder", "folder", params.kind.as_deref() == Some("folder")),
                            ] {
                                option value=(value) selected[selected] { (label) }
                            }
                        }
                        button type="submit" { "Look up" }
                    }
                    (result_section(&params))
                }
                section {
                    h2 { "Icon set" }
                    (icon_grid()?)
                }
            }
        }
    };
    Ok(Html(body))
}

fn result_section(params: &IndexParams) -> Markup {
    let Some(path) = params.path.as_deref().filter(|p| !p.is_empty()) else {
        return html! {};
    };

    let kind = Kind::from_query(params.kind.as_deref());
    let api_path = match kind {
        Kind::File => "/api/icon/file",
        Kind::Folder => "/api/icon/folder",
    };
    let api_url = format!(
        "{api_path}?path={}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(path.as_bytes())
    );

    match lookup(kind, path) {
        Ok(icon_id) => html! {
            div.result {
                img src=(format!("/icons/{icon_id}.svg")) alt=(format!("icon {icon_id}"))
                    width="64" height="64";
                p {
                    code { (path) }
                    " resolves to icon "
                    strong { (icon_id) }
                }
                p {
                    "API URL: "
                    code { (api_url) }
                }
            }
        },
        Err(ApiError::NoIcon) => html! {
            p.result.miss { "No icon found for " code { (path) } " — try another path." }
        },
        Err(ApiError::MissingPath | ApiError::InvalidBase64 | ApiError::Internal) => html! {
            p.result.miss { "Internal server error" }
        },
    }
}

/// The directory listing of `./icons`, read once per process and sorted
/// numerically by icon ID. Read errors are deployment errors (the SVG route
/// would be broken too), so they surface as 500.
fn icon_ids() -> Result<&'static Vec<u64>, ApiError> {
    static IDS: OnceLock<Vec<u64>> = OnceLock::new();

    if let Some(ids) = IDS.get() {
        return Ok(ids);
    }

    let entries = std::fs::read_dir("./icons").map_err(|err| {
        log::error!("cannot read ./icons directory: {err}");
        ApiError::Internal
    })?;

    let mut ids: Vec<u64> = entries
        .filter_map(|entry| {
            let name = entry.ok()?.file_name();
            let path = Path::new(&name);
            if path.extension().is_some_and(|ext| ext == "svg") {
                path.file_stem()?.to_str()?.parse::<u64>().ok()
            } else {
                None
            }
        })
        .collect();
    ids.sort_unstable();

    let _ = IDS.set(ids);
    IDS.get().ok_or(ApiError::Internal)
}

fn icon_grid() -> Result<Markup, ApiError> {
    let ids = icon_ids()?;
    Ok(html! {
        div.grid {
            @for &id in ids {
                div.cell {
                    img src=(format!("/icons/{id}.svg")) alt=(format!("icon {id}"))
                        width="48" height="48";
                    span { (id) }
                }
            }
        }
    })
}

const CSS: &str = r#"
body { font-family: system-ui, sans-serif; max-width: 900px; margin: 0 auto; padding: 1rem; }
section { margin-top: 2rem; }
form { display: flex; gap: 0.5rem; }
form input[type=text] { flex: 1; padding: 0.4rem; }
.result { margin-top: 1rem; }
.result img { vertical-align: middle; }
.miss { color: #b00; }
.grid { display: flex; flex-wrap: wrap; gap: 0.5rem; }
.cell { display: flex; flex-direction: column; align-items: center; min-width: 64px; padding: 0.4rem; border: 1px solid #eee; border-radius: 4px; }
.cell span { font-family: monospace; font-size: 0.8rem; color: #555; }
"#;
