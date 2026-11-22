use std::time::Duration;

use serde_json::Value;

use crate::Services;

const MIN_START: f64 = 8.0;
const MIN_END: f64 = 5.0;
const RUN: usize = 3;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Silence {
    pub start: f64,
    pub end: Option<f64>,
}

/// Finds leading and trailing silence from a waveform's amplitude samples.
pub fn silence_from_samples(samples: &[f64], length: f64) -> Option<Silence> {
    if samples.len() < 10 || length <= 0.0 {
        return None;
    }
    let peak = samples.iter().copied().fold(f64::MIN, f64::max);
    let limit = (peak * 0.03).max(2.0);
    let loud = |i: usize| samples[i] > limit;
    let solid = |i: usize| (0..RUN).all(|k| i + k < samples.len() && loud(i + k));
    let seconds_per = length / samples.len() as f64;

    let first = (0..samples.len()).find(|&i| solid(i))?;
    let mut last = samples.len() - 1;
    while last > first && !loud(last) {
        last -= 1;
    }

    let start = first as f64 * seconds_per;
    let end = (last + 1) as f64 * seconds_per;
    Some(Silence {
        start: if start >= MIN_START {
            (start - 0.3).max(0.0)
        } else {
            0.0
        },
        end: (length - end >= MIN_END).then_some(end + 0.3),
    })
}

impl Services {
    pub async fn waveform_silence(&self, track: &Value) -> Option<Silence> {
        let waveform = track["waveform_url"].as_str()?;
        let url = waveform
            .strip_suffix(".png")
            .map_or_else(|| waveform.to_string(), |base| format!("{base}.json"));
        let length = track["duration"].as_f64().unwrap_or(0.0) / 1000.0;
        if length == 0.0 {
            return None;
        }
        let res = self
            .inner
            .http
            .get(url)
            .timeout(Duration::from_secs(8))
            .send()
            .await
            .ok()?;
        if !res.status().is_success() {
            return None;
        }
        let json: Value = res.json().await.ok()?;
        let samples: Vec<f64> = json["samples"]
            .as_array()?
            .iter()
            .filter_map(Value::as_f64)
            .collect();
        silence_from_samples(&samples, length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wave(lead: usize, body: usize, tail: usize) -> Vec<f64> {
        [vec![0.0; lead], vec![50.0; body], vec![0.0; tail]].concat()
    }

    #[test]
    fn detects_long_lead_and_tail() {
        let samples = wave(40, 120, 40);
        let s = silence_from_samples(&samples, 200.0).unwrap();
        assert!((s.start - 39.7).abs() < 1e-9);
        assert!((s.end.unwrap() - 160.3).abs() < 1e-9);
    }

    #[test]
    fn ignores_short_silence() {
        let samples = wave(2, 190, 2);
        let s = silence_from_samples(&samples, 200.0).unwrap();
        assert_eq!(
            s,
            Silence {
                start: 0.0,
                end: None
            }
        );
    }

    #[test]
    fn needs_a_solid_run_and_enough_samples() {
        assert_eq!(silence_from_samples(&[0.0; 50], 100.0), None);
        assert_eq!(silence_from_samples(&[50.0; 5], 100.0), None);
        let spiky: Vec<f64> = (0..50)
            .map(|i| if i % 2 == 0 { 50.0 } else { 0.0 })
            .collect();
        assert_eq!(silence_from_samples(&spiky, 100.0), None);
    }
}
