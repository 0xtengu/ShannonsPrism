// Copyright (C) 2026 0xtengu
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 2 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

//! Metrics and scoring; combines size with empirical order-0 entropy so simple
//! alphabet expansions such as hex do not win on per-byte entropy alone.

use crate::codecs::metricsOf;

/// Shannon entropy alone, for the sliding-window map.
pub fn entropyOf(data: &[u8]) -> f64
{
    metricsOf(data).0
}

/// Highest entropy `n` uniform-random bytes can be expected to reach: `8 - 255/(2 n ln 2)`, capped by log2(n).
pub fn achievableEntropy(n: usize) -> f64
{
    if n == 0
    {
        return 0.0;
    }
    let corrected = 8.0 - 255.0 / (2.0 * n as f64 * std::f64::consts::LN_2);
    let alphabet_limit = (n as f64).log2().min(8.0);
    // Below ~40 bytes the correction goes negative and the alphabet limit binds; clamping to 0
    // instead gave a zero ceiling and classified random data as "structured".
    if corrected <= 0.0
    {
        alphabet_limit
    }
    else
    {
        corrected.min(alphabet_limit)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Metrics
{
    pub size: usize,
    pub entropy: f64,
    pub printable: f64,
    pub unique: usize,
}

impl Metrics
{
    pub fn of(data: &[u8]) -> Self
    {
        let (entropy, printable, _ascii, _nul, unique) = metricsOf(data);
        Metrics {
            size: data.len(),
            entropy,
            printable,
            unique,
        }
    }

    /// Empirical order-0 information estimate in bits.
    ///
    /// This is useful for comparing simple alphabet re-encodings, but is not
    /// invariant under every reversible transform and is not Kolmogorov complexity.
    pub fn infoBits(&self) -> f64
    {
        self.size as f64 * self.entropy
    }

    /// Entropy as a fraction of the maximum its alphabet allows; a hex dump scores ~1.0 at H = 4.0.
    pub fn entropyNorm(&self) -> f64
    {
        if self.unique <= 1
        {
            0.0
        }
        else
        {
            self.entropy / (self.unique as f64).log2()
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict
{
    /// Smaller, carries less information, entropy inside the band.
    Hit,
    /// Real compression, but entropy above the ceiling.
    OverH,
    /// Entropy looks low; information content is unchanged.
    Recode,
    /// Output is not smaller than the input.
    Expand,
    /// Failed round-trip, or produced nothing. Its ratio means nothing.
    Broken,
}

impl Verdict
{
    pub fn rank(self) -> u8
    {
        match self
        {
            Verdict::Hit => 0,
            Verdict::OverH => 1,
            Verdict::Recode => 2,
            Verdict::Expand => 3,
            Verdict::Broken => 4,
        }
    }

    pub fn label(self) -> &'static str
    {
        match self
        {
            Verdict::Hit => "hit",
            Verdict::OverH => "over-H",
            Verdict::Recode => "recode",
            Verdict::Expand => "expand",
            Verdict::Broken => "BROKEN",
        }
    }
}

/// Information content within this fraction of the original is a re-spelling, not compression.
const RECODE_THRESHOLD: f64 = 0.98;

/// How close H must sit to its alphabet ceiling before we call it structural.
const CAP_THRESHOLD: f64 = 0.995;

pub struct Row
{
    pub name: String,
    pub metrics: Metrics,
    pub ratio: f64,
    pub info_ratio: f64,
    pub verdict: Verdict,
    pub score: f64,
    pub capped: bool,
    pub elapsed: f64,
    pub note: String,
}

pub fn score(
    name: String,
    data: Vec<u8>,
    elapsed: f64,
    ok: bool,
    error: String,
    original: &Metrics,
    target_high: f64,
) -> Row
{
    let m = Metrics::of(&data);
    let ratio = if original.size > 0
    {
        m.size as f64 / original.size as f64
    }
    else
    {
        0.0
    };
    let info_ratio = if original.infoBits() > 0.0
    {
        m.infoBits() / original.infoBits()
    }
    else
    {
        0.0
    };

    let mut note = error.clone();
    let mut capped = false;

    if m.unique > 1 && m.entropyNorm() >= CAP_THRESHOLD
    {
        capped = true;
        if m.unique < 256 && note.is_empty()
        {
            note = format!(
                "H capped by a {}-symbol alphabet ({:.3} max)",
                m.unique,
                (m.unique as f64).log2()
            );
        }
    }

    let verdict = if !ok
    {
        Verdict::Broken
    }
    else if original.size > 0 && m.size == 0
    {
        // H(b"") and the ratio are both 0.0, so empty output would otherwise score perfectly.
        note = "empty output for non-empty input".into();
        Verdict::Broken
    }
    else if ratio >= 1.0
    {
        Verdict::Expand
    }
    else if capped && m.unique < 256
    {
        Verdict::Recode
    }
    else if info_ratio >= RECODE_THRESHOLD
    {
        if note.is_empty()
        {
            note = format!(
                "information content unchanged ({:.3} of original)",
                info_ratio
            );
        }
        Verdict::Recode
    }
    else if m.entropy > target_high
    {
        Verdict::OverH
    }
    else
    {
        Verdict::Hit
    };

    // Lower is better.
    let mut s = info_ratio;
    s += (ratio - 1.0).max(0.0) * 2.0;
    s += (m.entropy - target_high).max(0.0) / 8.0;
    if verdict == Verdict::Broken
    {
        s = f64::INFINITY;
    }

    drop(data);

    Row {
        name,
        metrics: m,
        ratio,
        info_ratio,
        verdict,
        score: s,
        capped,
        elapsed,
        note,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey
{
    Score,
    Info,
    Ratio,
    Size,
    Entropy,
    Time,
    Name,
}

pub fn sortRows(rows: &mut [Row], key: SortKey)
{
    let cmp = |a: f64, b: f64| a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal);
    match key
    {
        SortKey::Score => rows.sort_by(|a, b| {
            a.verdict
                .rank()
                .cmp(&b.verdict.rank())
                .then(cmp(a.score, b.score))
                .then(a.name.cmp(&b.name))
        }),
        SortKey::Info => rows.sort_by(|a, b| {
            a.verdict
                .rank()
                .cmp(&b.verdict.rank())
                .then(cmp(a.info_ratio, b.info_ratio))
                .then(a.name.cmp(&b.name))
        }),
        SortKey::Ratio => rows.sort_by(|a, b| {
            a.verdict
                .rank()
                .cmp(&b.verdict.rank())
                .then(cmp(a.ratio, b.ratio))
                .then(a.name.cmp(&b.name))
        }),
        SortKey::Size => rows.sort_by(|a, b| a.metrics.size.cmp(&b.metrics.size)),
        SortKey::Entropy => rows.sort_by(|a, b| cmp(a.metrics.entropy, b.metrics.entropy)),
        SortKey::Time => rows.sort_by(|a, b| cmp(b.elapsed, a.elapsed)),
        SortKey::Name => rows.sort_by(|a, b| a.name.cmp(&b.name)),
    }
}
