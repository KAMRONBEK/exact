//! Sheet detent parsing and content-height ceiling resolution (LLP 1075.003 §9.11, §9.13).

/// Whether `detent` names `fit-content`.
pub(super) fn fits(detent: Option<&str>) -> bool {
    detent.is_some_and(|d| d.split_whitespace().any(|w| w == "fit-content"))
}

/// Resolve `detent` into ascending `(alias_words, height)` stops plus any
/// preceding stop words omitted because a terminal `fit-content` ceiling lay at
/// or below them. `natural_tallest` / `natural_foot` / `natural_fit` exclude
/// keyboard shrinkage and IME lift so opening the keyboard on a short sheet
/// never introduces a spurious stop; `tallest` / `foot` / `display_fit` produce
/// the rendered heights. Stops that tie within 0.5 dp share one entry keeping
/// every collapsed word so initial/resting-stop lookup stays exact.
pub(super) fn resolve_stops(
    detent: Option<&str>,
    tallest: f32,
    foot: f32,
    natural_tallest: f32,
    natural_foot: f32,
    natural_fit: f32,
    display_fit: f32,
) -> (Vec<(Vec<String>, f32)>, Vec<String>) {
    #[derive(Clone, Copy, PartialEq)]
    enum Kind {
        Points,
        Share,
        Content,
        Large,
    }
    let mut kept: Vec<(&str, Kind, f32)> = Vec::new();
    for word in detent.unwrap_or("large").split_whitespace() {
        let parsed = match word {
            "large" => Some((Kind::Large, 100.0)),
            "medium" => Some((Kind::Share, 50.0)),
            "fit-content" => Some((Kind::Content, 0.0)),
            d => match d.strip_suffix('%') {
                Some(p) => p
                    .parse::<f32>()
                    .ok()
                    .filter(|p| p.is_finite() && *p > 0.0 && *p <= 100.0)
                    .map(|p| {
                        if p >= 100.0 {
                            (Kind::Large, 100.0)
                        } else {
                            (Kind::Share, p)
                        }
                    }),
                None => d
                    .parse::<f32>()
                    .ok()
                    .filter(|v| v.is_finite() && *v > 0.0)
                    .map(|v| (Kind::Points, v)),
            },
        };
        let Some((kind, raw)) = parsed else {
            continue;
        };
        let ok = match kept.last() {
            None => true,
            Some((_, last_kind, last_raw)) => match (*last_kind, kind) {
                (Kind::Large, _) => false,
                (Kind::Content, Kind::Large) => kept.len() == 1,
                (Kind::Content, _) => false,
                (_, Kind::Large | Kind::Content) => true,
                (a, b) => a == b && raw > *last_raw,
            },
        };
        if ok {
            kept.push((word, kind, raw));
        }
    }
    let cap = tallest.max(1.0);
    let nat_cap = natural_tallest.max(1.0);
    let nat_fit = natural_fit.clamp(1.0, nat_cap);
    let disp_fit = display_fit.clamp(1.0, cap);
    let resolve_in = |kind: Kind, raw: f32, max_h: f32, pad: f32, fit: f32| -> f32 {
        match kind {
            Kind::Large => max_h,
            Kind::Share => (max_h * raw / 100.0 + pad).clamp(1.0, max_h),
            Kind::Points => (raw + pad).clamp(1.0, max_h),
            Kind::Content => fit,
        }
    };
    let push_stop = |out: &mut Vec<(Vec<String>, f32)>, word: &str, h: f32| {
        if let Some(prev) = out.last_mut() {
            if (h - prev.1).abs() < 0.5 {
                prev.0.push(word.to_string());
                return;
            }
        }
        out.push((vec![word.to_string()], h));
    };
    let terminal_fit = kept.len() > 1 && kept.last().is_some_and(|(_, k, _)| *k == Kind::Content);
    let mut out: Vec<(Vec<String>, f32)> = Vec::new();
    let mut dropped: Vec<String> = Vec::new();
    if terminal_fit {
        for &(word, kind, raw) in &kept[..kept.len() - 1] {
            let nat_h = resolve_in(kind, raw, nat_cap, natural_foot, nat_fit);
            if nat_h < nat_fit - 0.5 {
                let disp_h = resolve_in(kind, raw, cap, foot, disp_fit);
                push_stop(&mut out, word, disp_h);
            } else {
                dropped.push(word.to_string());
            }
        }
        push_stop(&mut out, "fit-content", disp_fit);
    } else {
        for &(word, kind, raw) in &kept {
            let h = resolve_in(kind, raw, cap, foot, disp_fit);
            push_stop(&mut out, word, h);
        }
    }
    if out.is_empty() {
        out.push((vec!["large".to_string()], cap));
    }
    (out, dropped)
}
