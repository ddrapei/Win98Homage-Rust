//! A city as somebody types it, looked up: Open-Meteo's geocoder turns the
//! name into a place -- what it is called, where it is, and which time zone
//! it keeps.  The weather wants the second and the world clock the third.
//!
//! The answer is JSON, and is read the way everything from the network is
//! here: by looking for the keys that are wanted.

use crate::http;

/// The text after `key` in `s`.
pub fn after<'a>(s: &'a str, key: &str) -> Option<&'a str> {
    s.find(key).map(|i| &s[i + key.len()..])
}

/// The text up to the first of `ends`.
pub fn until<'a>(s: &'a str, ends: &[char]) -> &'a str {
    s[..s.find(ends).unwrap_or(s.len())].trim()
}

/// A place the geocoder knows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub name: String,
    /// Its country's two-letter code, or nothing.
    pub country: String,
    /// Decimal degrees, as the geocoder wrote them.
    pub lat: String,
    pub lon: String,
    /// The time zone it keeps, by its tz database name: "Europe/Kyiv".
    pub zone: String,
}

impl Place {
    /// "Lviv, UA"
    pub fn label(&self) -> String {
        if self.country.is_empty() { self.name.clone() } else { format!("{}, {}", self.name, self.country) }
    }
}

/// Why there is no place to report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Miss {
    /// The geocoder could not be asked.
    NoAnswer,
    /// It was, and knows no such place.
    NoCity,
}

/// What was typed, as the name to search for and what was said about where:
/// a two-letter country code ("Paris, US"), or a region or country in words
/// ("Portland, Maine").  Only the code can be asked of the geocoder; the
/// words are for choosing among what it answers.
pub fn split_query(typed: &str) -> (&str, Option<String>, &str) {
    let Some((name, place)) = typed.split_once(',') else { return (typed.trim(), None, "") };
    let (name, place) = (name.trim(), place.trim());
    if place.len() == 2 && place.bytes().all(|b| b.is_ascii_alphabetic()) {
        let code = place.to_ascii_uppercase();
        // the one country everybody writes by something other than its code
        return (name, Some(if code == "UK" { String::from("GB") } else { code }), "");
    }
    (name, None, place)
}

/// Everything but the unreserved characters as %XX, for a query string.
pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A place out of the geocoder's answer: the first of its results, or --
/// when `region` names a state or a country -- the first from there, if
/// there is one.  The results are flat objects, most likely first, so they
/// come apart at "},{".
pub fn parse(json: &str, region: &str) -> Option<Place> {
    let text = |item: &str, key: &str| after(item, key).map(|s| until(s, &['"']).to_owned());
    let mut first = None;
    for item in after(json, "\"results\":[")?.split("},{") {
        let number = |key: &str| after(item, key).map(|s| until(s, &[',', '}']).to_owned()).filter(|v| v.parse::<f64>().is_ok());
        let (Some(name), Some(lat), Some(lon)) = (text(item, "\"name\":\""), number("\"latitude\":"), number("\"longitude\":")) else {
            continue;
        };
        let place = Place {
            name,
            country: text(item, "\"country_code\":\"").unwrap_or_default(),
            lat,
            lon,
            // a JSON writer may put a backslash before the slash in it
            zone: text(item, "\"timezone\":\"").unwrap_or_default().replace('\\', ""),
        };
        let is_there = |key: &str| text(item, key).is_some_and(|v| v.to_lowercase() == region.to_lowercase());
        if region.is_empty() || is_there("\"admin1\":\"") || is_there("\"country\":\"") {
            return Some(place);
        }
        first.get_or_insert(place);
    }
    first
}

/// Ask the geocoder for what was typed.  This is the network: it is for a
/// child to call.
pub fn find(typed: &str) -> Result<Place, Miss> {
    let (name, country, region) = split_query(typed);
    // one result is enough unless there is a region to look for among them
    let mut path = format!(
        "/v1/search?name={}&count={}&language=en&format=json",
        url_encode(name),
        if region.is_empty() { 1 } else { 20 }
    );
    if let Some(code) = country {
        path.push_str("&countryCode=");
        path.push_str(&code);
    }
    let json = http::get("geocoding-api.open-meteo.com", 443, true, &path).ok_or(Miss::NoAnswer)?;
    parse(&json, region).ok_or(Miss::NoCity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_city() {
        let json = r#"{"results":[{"id":1,"name":"Portland","latitude":45.52345,"longitude":-122.67621,"elevation":15.0,"feature_code":"PPLA2","country_code":"US","admin1_id":5744337,"timezone":"America/Los_Angeles","postcodes":["97201","97202"],"country_id":6252001,"country":"United States","admin1":"Oregon","admin2":"Multnomah"},{"id":2,"name":"Portland","latitude":43.65737,"longitude":-70.2589,"country_code":"US","timezone":"America\/New_York","country":"United States","admin1":"Maine"}],"generationtime_ms":0.2}"#;
        let place = |lat: &str, lon: &str, zone: &str| {
            Some(Place { name: "Portland".into(), country: "US".into(), lat: lat.into(), lon: lon.into(), zone: zone.into() })
        };
        let oregon = place("45.52345", "-122.67621", "America/Los_Angeles");
        assert_eq!(parse(json, ""), oregon);
        assert_eq!(parse(json, "maine"), place("43.65737", "-70.2589", "America/New_York"));
        assert_eq!(parse(json, "united states"), oregon);
        // a region none of them is in: the likeliest is still an answer
        assert_eq!(parse(json, "Texas"), oregon);
        assert_eq!(oregon.unwrap().label(), "Portland, US");
        // no match at all comes back with no results key
        assert_eq!(parse(r#"{"generationtime_ms":0.1}"#, ""), None);
    }

    #[test]
    fn splits_what_was_typed() {
        assert_eq!(split_query(" Lviv "), ("Lviv", None, ""));
        assert_eq!(split_query("paris, us"), ("paris", Some("US".to_owned()), ""));
        assert_eq!(split_query("London,UK"), ("London", Some("GB".to_owned()), ""));
        assert_eq!(split_query("Portland, Maine"), ("Portland", None, "Maine"));
    }

    #[test]
    fn encodes_a_name_for_the_query() {
        assert_eq!(url_encode("New York"), "New%20York");
        assert_eq!(url_encode("M\u{fc}nchen&x=1"), "M%C3%BCnchen%26x%3D1");
    }
}
