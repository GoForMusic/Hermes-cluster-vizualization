//! Kubernetes quantities: `100m`, `512Mi`, `2Gi`, `1.5`, `129e6`. The API sends CPU and memory as strings.

/// The value in base units (cores, bytes), or `None` when the text is not a quantity.
pub fn parse(text: &str) -> Option<f64> {
    let s = text.trim();
    let split = s
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '+' | '-')))
        .unwrap_or(s.len());
    let (number, suffix) = s.split_at(split);
    let value: f64 = number.parse().ok()?;
    let factor = match suffix {
        "" => 1.0,
        "n" => 1e-9,
        "u" => 1e-6,
        "m" => 1e-3,
        "k" => 1e3,
        "M" => 1e6,
        "G" => 1e9,
        "T" => 1e12,
        "P" => 1e15,
        "E" => 1e18,
        "Ki" => 1024.0,
        "Mi" => 1024.0_f64.powi(2),
        "Gi" => 1024.0_f64.powi(3),
        "Ti" => 1024.0_f64.powi(4),
        "Pi" => 1024.0_f64.powi(5),
        "Ei" => 1024.0_f64.powi(6),
        exp if exp.starts_with(['e', 'E']) => 10.0_f64.powi(exp[1..].parse().ok()?),
        _ => return None,
    };
    Some(value * factor)
}

/// CPU in millicores.
pub fn milli(text: &str) -> Option<f64> {
    parse(text).map(|v| (v * 1000.0).round())
}

#[cfg(test)]
#[path = "../../tests/unit/k8s_quantity.rs"]
mod tests;
