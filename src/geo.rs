//! Ortsname zu GPS-Koordinaten, komplett offline.
//!
//! Daten: alle Orte ab 5.000 Einwohnern aus GeoNames (geonames.org,
//! CC BY 4.0), mit deutschen Namen wo vorhanden ("Lissabon", "München").
//! Aufbereitet als `data/orte.tsv.gz`: Name, Breite, Länge – nach Breite
//! sortiert, damit nur ein schmaler Streifen durchsucht werden muss.

use std::{io::Read, sync::OnceLock};

static DATA: &[u8] = include_bytes!("../data/orte.tsv.gz");
/// weiter weg zählt nicht mehr als „dort“
const MAX_KM: f64 = 30.0;

struct Place {
    name: String,
    lat: f64,
    lon: f64,
}

fn places() -> &'static [Place] {
    static PLACES: OnceLock<Vec<Place>> = OnceLock::new();
    PLACES.get_or_init(|| {
        let mut text = String::new();
        if flate2::read::GzDecoder::new(DATA).read_to_string(&mut text).is_err() {
            return Vec::new();
        }
        let mut v: Vec<Place> = text
            .lines()
            .filter_map(|l| {
                let mut f = l.split('\t');
                Some(Place { name: f.next()?.to_owned(), lat: f.next()?.parse().ok()?, lon: f.next()?.parse().ok()? })
            })
            .collect();
        v.sort_by(|a, b| a.lat.total_cmp(&b.lat));
        v
    })
}

fn km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let (dp, dl) = ((lat2 - lat1).to_radians(), (lon2 - lon1).to_radians());
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    6371.0 * 2.0 * a.sqrt().asin()
}

/// Nächster Ort im Umkreis von 30 km.
pub fn place_name(lat: f64, lon: f64) -> Option<&'static str> {
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) || (lat == 0.0 && lon == 0.0) {
        return None;
    }
    let all = places();
    // 30 km sind höchstens ca. 0,28° Breite
    let band = MAX_KM / 111.0;
    let start = all.partition_point(|p| p.lat < lat - band);
    all[start..]
        .iter()
        .take_while(|p| p.lat <= lat + band)
        .map(|p| (km(lat, lon, p.lat, p.lon), p))
        .filter(|(d, _)| *d <= MAX_KM)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, p)| p.name.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_german_names() {
        assert_eq!(place_name(38.7223, -9.1393), Some("Lissabon"));
        assert_eq!(place_name(48.1372, 11.5756), Some("München"));
        // mitten im Atlantik
        assert_eq!(place_name(40.0, -40.0), None);
        assert_eq!(place_name(0.0, 0.0), None);
    }
}
