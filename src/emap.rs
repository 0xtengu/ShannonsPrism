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

//! Sliding-window entropy map: per-block entropy, coalesced into labelled regions.

use crate::score::{achievableEntropy, entropyOf};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Band
{
    /// H < 1: runs of a single value — padding, sparse memory, alignment.
    Padding,
    /// 1 <= H < 5: structured data, tables, headers.
    Structured,
    /// 5 <= H < 7: native code.
    Code,
    /// 7 <= H < 7.9: high entropy, often compressed.
    Compressed,
    /// H >= 7.9: statistically close to random; cause is not implied.
    Random,
}

/// Fewest bytes from which a band can be judged; below this sampling noise swamps the signal.
pub const MIN_JUDGEABLE: usize = 256;

impl Band
{
    pub fn of(h: f64) -> Band
    {
        if h < 1.0
        {
            Band::Padding
        }
        else if h < 5.0
        {
            Band::Structured
        }
        else if h < 7.0
        {
            Band::Code
        }
        else if h < 7.9
        {
            Band::Compressed
        }
        else
        {
            Band::Random
        }
    }
    /// Classify entropy measured over `n` bytes, scaled against the ceiling that window can reach.
    pub fn ofWindowed(h: f64, n: usize) -> Band
    {
        let ceiling = achievableEntropy(n);
        if ceiling <= 0.0
        {
            return Band::of(h);
        }
        Band::of((h * 8.0 / ceiling).min(8.0))
    }

    /// Classify a whole section, or None when it is too small to judge.
    pub fn classifySection(h: f64, n: usize) -> Option<Band>
    {
        if n < MIN_JUDGEABLE
        {
            None
        }
        else
        {
            Some(Band::ofWindowed(h, n))
        }
    }
    pub fn label(self) -> &'static str
    {
        match self
        {
            Band::Padding => "padding",
            Band::Structured => "structured",
            Band::Code => "code/text",
            Band::Compressed => "compressed",
            Band::Random => "random",
        }
    }
    pub fn glyph(self) -> char
    {
        match self
        {
            Band::Padding => ' ',
            Band::Structured => '\u{2591}', // light shade
            Band::Code => '\u{2592}',       // medium shade
            Band::Compressed => '\u{2593}', // dark shade
            Band::Random => '\u{2588}',     // full block
        }
    }

    /// Refine a high-entropy band by histogram uniformity without claiming a cause.
    pub fn describe(self, u: Option<Uniformity>) -> &'static str
    {
        match (self, u)
        {
            (Band::Random, Some(u)) if u.isUniform() => "uniform-like high entropy",
            (Band::Random, Some(_)) => "nonuniform high entropy",
            _ => self.label(),
        }
    }
}

/// Chi-square test of a byte histogram against the uniform distribution.
#[derive(Clone, Copy, Debug)]
pub struct Uniformity
{
    /// The statistic, 255 degrees of freedom. Uniform data lands near 255.
    pub chi2: f64,
    /// Probability of a statistic at least this large under uniformity.
    pub p: f64,
}

impl Uniformity
{
    /// Below this p the histogram is called non-uniform.
    pub const ALPHA: f64 = 0.001;

    pub fn isUniform(&self) -> bool
    {
        self.p >= Self::ALPHA
    }

    /// Short rendering for a table cell or a region note.
    pub fn summary(&self) -> String
    {
        if self.p < 0.001
        {
            format!("chi2 {:.0}, p<0.001", self.chi2)
        }
        else
        {
            format!("chi2 {:.0}, p={:.2}", self.chi2, self.p)
        }
    }
}

/// Below this the expected count per bin drops under 1 and the chi-square approximation fails.
pub const MIN_CHI2_BYTES: usize = 256;

pub fn uniformity(data: &[u8]) -> Uniformity
{
    let n = data.len();
    let mut hist = [0u64; 256];
    for &b in data
    {
        hist[b as usize] += 1;
    }
    let expected = n as f64 / 256.0;
    let chi2 = if expected > 0.0
    {
        hist.iter()
            .map(|&o| {
                let d = o as f64 - expected;
                d * d / expected
            })
            .sum()
    }
    else
    {
        0.0
    };
    Uniformity {
        chi2,
        p: chi2Survival(chi2, 255.0),
    }
}

/// Upper tail of the chi-square distribution via the Wilson-Hilferty normal approximation (k >= 30).
fn chi2Survival(x: f64, k: f64) -> f64
{
    if x <= 0.0
    {
        return 1.0;
    }
    let c = 2.0 / (9.0 * k);
    let z = ((x / k).cbrt() - (1.0 - c)) / c.sqrt();
    0.5 * erfc(z / std::f64::consts::SQRT_2)
}

/// Complementary error function, Abramowitz & Stegun 7.1.26 (|err| < 1.5e-7).
fn erfc(x: f64) -> f64
{
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let poly = t
        * (0.254829592
            + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
    let e = poly * (-x * x).exp();
    if x >= 0.0
    {
        e
    }
    else
    {
        2.0 - e
    }
}

pub struct Block
{
    pub offset: usize,
    pub entropy: f64,
}

pub struct Region
{
    pub start: usize,
    pub end: usize,
    pub band: Band,
    pub mean_entropy: f64,
    /// Chi-square verdict, computed for Random regions large enough to test.
    pub uniformity: Option<Uniformity>,
}

impl Region
{
    pub fn len(&self) -> usize
    {
        self.end.saturating_sub(self.start)
    }
    /// "encrypted", "packed", or the plain band name.
    pub fn describe(&self) -> &'static str
    {
        self.band.describe(self.uniformity)
    }
}

pub struct EntropyMap
{
    pub window: usize,
    pub step: usize,
    pub blocks: Vec<Block>,
    pub regions: Vec<Region>,
}

/// Compute the block map and coalesce into regions.
pub fn compute(data: &[u8], window: usize, step: usize) -> EntropyMap
{
    let window = window.max(16).min(data.len().max(16));
    let step = step.clamp(1, window);

    let mut blocks = Vec::new();
    let mut off = 0usize;
    while off < data.len()
    {
        let end = (off + window).min(data.len());
        blocks.push(Block {
            offset: off,
            entropy: entropyOf(&data[off..end]),
        });
        if end == data.len()
        {
            break;
        }
        off += step;
    }

    let mut regions = Vec::new();
    if !blocks.is_empty()
    {
        let mut start = 0usize;
        let mut sum = blocks[0].entropy;
        let mut count = 1usize;
        let mut band = Band::ofWindowed(blocks[0].entropy, window);
        for i in 1..blocks.len()
        {
            let b = Band::ofWindowed(blocks[i].entropy, window);
            if b == band
            {
                sum += blocks[i].entropy;
                count += 1;
            }
            else
            {
                regions.push(Region {
                    start: blocks[start].offset,
                    end: blocks[i].offset,
                    band,
                    mean_entropy: sum / count as f64,
                    uniformity: None,
                });
                start = i;
                sum = blocks[i].entropy;
                count = 1;
                band = b;
            }
        }
        let last_end = (blocks[start].offset + window).min(data.len());
        regions.push(Region {
            start: blocks[start].offset,
            end: last_end.max(blocks.last().unwrap().offset),
            band,
            mean_entropy: sum / count as f64,
            uniformity: None,
        });
    }

    for r in regions.iter_mut()
    {
        if r.band == Band::Random && r.len() >= MIN_CHI2_BYTES
        {
            r.uniformity = Some(uniformity(&data[r.start..r.end.min(data.len())]));
        }
    }

    EntropyMap {
        window,
        step,
        blocks,
        regions,
    }
}

/// Downsample the block series to `width` cells for a one-line strip render.
pub fn strip(map: &EntropyMap, width: usize) -> Vec<Band>
{
    if map.blocks.is_empty()
    {
        return Vec::new();
    }
    let width = width.max(1);
    let mut cells = Vec::with_capacity(width);
    let n = map.blocks.len();
    for i in 0..width
    {
        let lo = i * n / width;
        let hi = ((i + 1) * n / width).max(lo + 1).min(n);
        let mean = map.blocks[lo..hi].iter().map(|b| b.entropy).sum::<f64>() / (hi - lo) as f64;
        cells.push(Band::ofWindowed(mean, map.window));
    }
    cells
}
