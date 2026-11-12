//! Serde helper for `f64` fields that may be infinite or NaN (JSON has no
//! representation for them): written as the strings "inf", "-inf", "nan",
//! read back from those, from numbers, or from `null` (→ +inf).
//!
//! Use with `#[serde(with = "nr_core::serde_f64")]`.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub fn serialize<S: Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
    if v.is_finite() {
        v.serialize(s)
    } else if v.is_nan() {
        "nan".serialize(s)
    } else if *v > 0.0 {
        "inf".serialize(s)
    } else {
        "-inf".serialize(s)
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Repr {
    Num(f64),
    Text(String),
    Null(()),
}

pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    Ok(match Option::<Repr>::deserialize(d)? {
        Some(Repr::Num(v)) => v,
        Some(Repr::Text(t)) => match t.as_str() {
            "-inf" => f64::NEG_INFINITY,
            "nan" => f64::NAN,
            _ => f64::INFINITY,
        },
        Some(Repr::Null(())) | None => f64::INFINITY,
    })
}

#[cfg(test)]
mod tests {
    #[derive(serde::Serialize, serde::Deserialize)]
    struct T {
        #[serde(with = "super")]
        v: f64,
    }

    #[test]
    fn round_trips_non_finite() {
        for v in [1.5, f64::INFINITY, f64::NEG_INFINITY] {
            let j = serde_json::to_string(&T { v }).unwrap();
            let back: T = serde_json::from_str(&j).unwrap();
            assert_eq!(back.v, v, "{j}");
        }
        let back: T = serde_json::from_str(r#"{"v":null}"#).unwrap();
        assert!(back.v.is_infinite());
    }
}
