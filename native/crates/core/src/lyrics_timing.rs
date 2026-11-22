#[derive(Clone, Debug, PartialEq)]
pub struct LyricWord {
    pub t: f64,
    pub w: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LyricLine {
    pub t: f64,
    pub text: String,
    pub words: Option<Vec<LyricWord>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WordTiming {
    pub w: String,
    pub s: f64,
    pub e: f64,
}

pub fn clamp01(n: f64) -> f64 {
    n.clamp(0.0, 1.0)
}

fn js_len(s: &str) -> usize {
    s.encode_utf16().count()
}

pub fn build_timings(lines: &[LyricLine]) -> Vec<Vec<WordTiming>> {
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let next = lines.get(i + 1).map_or(line.t + 6.0, |l| l.t);

            if let Some(words) = line.words.as_ref().filter(|w| !w.is_empty()) {
                return words
                    .iter()
                    .enumerate()
                    .map(|(k, word)| WordTiming {
                        w: word.w.clone(),
                        s: word.t,
                        e: words
                            .get(k + 1)
                            .map_or_else(|| next.min(word.t + 0.8), |n| n.t),
                    })
                    .collect();
            }

            let words: Vec<&str> = line.text.split(' ').collect();
            let singing = (next - line.t - 0.15)
                .max(0.4)
                .min((js_len(&line.text) as f64 / 9.0).max(1.0));
            let weight: f64 = words.iter().map(|w| (js_len(w) + 1) as f64).sum();

            let mut at = line.t;
            words
                .into_iter()
                .map(|w| {
                    let span = ((js_len(w) + 1) as f64 / weight) * singing;
                    let timing = WordTiming {
                        w: w.to_string(),
                        s: at,
                        e: at + span,
                    };
                    at += span;
                    timing
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(t: f64, text: &str, words: Option<Vec<(f64, &str)>>) -> LyricLine {
        LyricLine {
            t,
            text: text.into(),
            words: words.map(|w| {
                w.into_iter()
                    .map(|(t, w)| LyricWord { t, w: w.into() })
                    .collect()
            }),
        }
    }

    #[test]
    fn word_stamped_lines() {
        let lines = [
            line(1.0, "a b", Some(vec![(1.0, "a"), (1.5, "b")])),
            line(5.0, "c", None),
        ];
        let t = build_timings(&lines);
        assert_eq!(
            t[0][0],
            WordTiming {
                w: "a".into(),
                s: 1.0,
                e: 1.5
            }
        );
        assert!((t[0][1].e - 2.3).abs() < 1e-9);
        let lines = [line(1.0, "a", Some(vec![(1.0, "a")])), line(1.4, "b", None)];
        assert_eq!(build_timings(&lines)[0][0].e, 1.4);
    }

    #[test]
    fn estimated_lines() {
        let lines = [line(10.0, "ab cde", None), line(12.0, "x", None)];
        let t = build_timings(&lines);
        let singing = (12.0f64 - 10.0 - 0.15)
            .max(0.4)
            .min((6.0f64 / 9.0).max(1.0));
        assert_eq!(singing, 1.0);
        assert!((t[0][0].e - t[0][0].s - 3.0 / 7.0).abs() < 1e-9);
        assert!((t[0][1].e - 11.0).abs() < 1e-9);
        assert_eq!(t[0][1].s, t[0][0].e);
        let last = build_timings(&[line(0.0, "solo", None)]);
        assert!((last[0][0].e - 1.0).abs() < 1e-9);
    }

    #[test]
    fn clamps() {
        assert_eq!(clamp01(-1.0), 0.0);
        assert_eq!(clamp01(2.0), 1.0);
    }
}
