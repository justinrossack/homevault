//! Netzwerkadapter des Servers (für „Backup/Sync über Adapter X“).
//!
//! Funktioniert, weil die Web-App im Netzwerk des Servers läuft
//! (`network_mode: host`). Damit die Verbindungen auch wirklich über den
//! gewählten Adapter gehen, wenn beide im selben Netz hängen, braucht der
//! Server eine Routing-Regel „von Adresse X über Adapter X“ (siehe README).

use std::net::Ipv4Addr;

use axum::{Extension, Json, extract::State};
use serde_json::{Value, json};

use crate::{AppState, auth::CurrentUser, error::AppError};

/// IPv4-Adressen aller Adapter: (Name, Adresse)
fn addresses() -> Vec<(String, Ipv4Addr)> {
    let mut out = Vec::new();
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs füllt eine verkettete Liste, die wir unten wieder freigeben
    if unsafe { libc::getifaddrs(&mut list) } != 0 {
        return out;
    }
    let mut cur = list;
    while !cur.is_null() {
        // SAFETY: Einträge der Liste sind gültig, bis freeifaddrs aufgerufen wird
        let ifa = unsafe { &*cur };
        if !ifa.ifa_addr.is_null() && unsafe { (*ifa.ifa_addr).sa_family } as i32 == libc::AF_INET {
            let sin = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in) };
            let name = unsafe { std::ffi::CStr::from_ptr(ifa.ifa_name) }.to_string_lossy().into_owned();
            out.push((name, Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr))));
        }
        cur = ifa.ifa_next;
    }
    unsafe { libc::freeifaddrs(list) };
    out
}

/// Erste IPv4-Adresse eines Adapters
pub fn ipv4_of(iface: &str) -> Option<Ipv4Addr> {
    addresses().into_iter().find(|(n, _)| n == iface).map(|(_, ip)| ip)
}

/// Interne Adapter von Docker & Co. zeigen wir nicht an
fn is_virtual(name: &str) -> bool {
    name == "lo" || ["docker", "br-", "veth", "virbr", "vnet", "tun", "tap", "wg"].iter().any(|p| name.starts_with(p))
}

fn sys(name: &str, file: &str) -> Option<String> {
    std::fs::read_to_string(format!("/sys/class/net/{name}/{file}")).ok().map(|s| s.trim().to_owned())
}

pub async fn list(State(_): State<AppState>, Extension(me): Extension<CurrentUser>) -> Result<Json<Value>, AppError> {
    if !me.admin {
        return Err(AppError::Forbidden);
    }
    let addrs = addresses();
    let mut names: Vec<String> = std::fs::read_dir("/sys/class/net")
        .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_else(|_| addrs.iter().map(|(n, _)| n.clone()).collect());
    names.sort();
    names.dedup();
    let adapters: Vec<Value> = names
        .into_iter()
        .filter(|n| !is_virtual(n))
        .map(|n| {
            let ip = addrs.iter().find(|(a, _)| *a == n).map(|(_, ip)| ip.to_string());
            // Geschwindigkeit ist -1, solange kein Kabel steckt
            let speed = sys(&n, "speed").and_then(|s| s.parse::<i64>().ok()).filter(|s| *s > 0);
            let wireless = std::path::Path::new(&format!("/sys/class/net/{n}/wireless")).exists();
            let usb = std::fs::read_link(format!("/sys/class/net/{n}/device")).is_ok_and(|p| p.to_string_lossy().contains("usb"));
            json!({
                "name": n,
                "ipv4": ip,
                "up": sys(&n, "operstate").as_deref() == Some("up"),
                "speed_mbit": speed,
                "mac": sys(&n, "address"),
                "kind": if wireless { "wlan" } else if usb { "usb" } else { "lan" },
            })
        })
        .collect();
    Ok(Json(json!({ "adapters": adapters })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_has_an_address() {
        assert!(addresses().iter().any(|(_, ip)| ip.is_loopback()));
        assert!(is_virtual("docker0") && is_virtual("veth12ab") && !is_virtual("enp0s25"));
    }
}
