//! Client for Caddy's admin API (HTTP+JSON on localhost:2019).
//!
//! Charm does exactly two things to Caddy: put a route on the server that owns
//! `:443` (creating one if none exists), and remove it. **Certificates are
//! Caddy's job** - public domains get ACME automatically; Charm never touches
//! TLS config or the Caddyfile.

use anyhow::{Context, Result};
use serde_json::{json, Value};

const ADMIN: &str = "http://127.0.0.1:2019";

fn route_id(app: &str) -> String {
    format!("charm_{app}")
}

pub fn publish(app: &str, host: &str, ip: &str, port: u16) -> Result<()> {
    let server = https_server()?;
    let id = route_id(app);

    // Idempotent: drop any previous route for this app first.
    let _ = ureq::delete(&format!("{ADMIN}/id/{id}")).call();

    let route = json!({
        "@id": id,
        "match": [{ "host": [host] }],
        "handle": [{
            "handler": "reverse_proxy",
            "upstreams": [{ "dial": format!("{ip}:{port}") }]
        }]
    });
    ureq::put(&format!("{ADMIN}/config/apps/http/servers/{server}/routes/0"))
        .send_json(route)
        .map_err(status_err)
        .context("publishing route to Caddy")?;
    Ok(())
}

pub fn unpublish(app: &str) -> Result<()> {
    let _ = ureq::delete(&format!("{ADMIN}/id/{}", route_id(app))).call();
    Ok(())
}

/// Is this app's route currently present in Caddy's live config?
pub fn is_published(app: &str) -> bool {
    ureq::get(&format!("{ADMIN}/id/{}", route_id(app)))
        .call()
        .is_ok()
}

/// Name of the server bound to `:443`. On a shared box that's the operator's
/// existing HTTPS server; if nothing serves 443 yet, create a dedicated one.
fn https_server() -> Result<String> {
    let servers: Value = ureq::get(&format!("{ADMIN}/config/apps/http/servers"))
        .call()
        .map_err(status_err)
        .context("reading Caddy servers")?
        .into_json()
        .unwrap_or_else(|_| json!({}));

    if let Some(obj) = servers.as_object() {
        for (name, srv) in obj {
            if listens_on_443(srv) {
                return Ok(name.clone());
            }
        }
    }
    ureq::put(&format!("{ADMIN}/config/apps/http/servers/charm"))
        .send_json(json!({ "listen": [":443"], "routes": [] }))
        .map_err(status_err)
        .context("creating an HTTPS server")?;
    Ok("charm".to_string())
}

fn listens_on_443(srv: &Value) -> bool {
    srv.get("listen")
        .and_then(|l| l.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .any(|s| s.ends_with(":443") || s == "443")
        })
        .unwrap_or(false)
}

fn status_err(e: ureq::Error) -> anyhow::Error {
    match e {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            anyhow::anyhow!("Caddy admin API returned {code}: {}", body.trim())
        }
        other => anyhow::anyhow!("Caddy admin API error: {other}"),
    }
}
