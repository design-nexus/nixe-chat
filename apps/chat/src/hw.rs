//! How much memory there is for models, and whether a download will fit.
//!
//! A sleeping discrete GPU is never woken to ask: NVIDIA memory is read with
//! `nvidia-smi` only while the card is awake, and remembered for next time.

use std::path::Path;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Memory {
    /// Dedicated GPU memory (0 when there's no discrete GPU we can read).
    pub vram: u64,
    pub ram: u64,
}

pub fn detect(cached_vram: u64) -> Memory {
    Memory { vram: amd_vram().or_else(nvidia_vram).unwrap_or(cached_vram), ram: ram() }
}

fn ram() -> u64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|t| {
            t.lines().find(|l| l.starts_with("MemTotal:")).and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
        })
        .map(|kb| kb * 1024)
        .unwrap_or(0)
}

fn amd_vram() -> Option<u64> {
    let entries = std::fs::read_dir("/sys/class/drm").ok()?;
    entries
        .flatten()
        .filter_map(|e| std::fs::read_to_string(e.path().join("device/mem_info_vram_total")).ok()?.trim().parse::<u64>().ok())
        // An APU reports a small carve-out; only count real cards.
        .filter(|v| *v >= 2 << 30)
        .max()
}

fn nvidia_vram() -> Option<u64> {
    let entries = std::fs::read_dir("/sys/bus/pci/devices").ok()?;
    let awake = entries.flatten().any(|e| {
        let vendor = std::fs::read_to_string(e.path().join("vendor")).unwrap_or_default();
        let class = std::fs::read_to_string(e.path().join("class")).unwrap_or_default();
        vendor.trim() == "0x10de" && class.starts_with("0x03") && runtime_active(&e.path())
    });
    if !awake || !crate::cmd::present("nvidia-smi") {
        return None;
    }
    let out = crate::cmd::output(&["nvidia-smi", "--query-gpu=memory.total", "--format=csv,noheader,nounits"])?;
    out.lines().filter_map(|l| l.trim().parse::<u64>().ok()).max().map(|mib| mib * 1024 * 1024)
}

fn runtime_active(dev: &Path) -> bool {
    std::fs::read_to_string(dev.join("power/runtime_status")).map(|s| s.trim() != "suspended").unwrap_or(true)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    Gpu,
    Partial,
    TooBig,
}

impl Fit {
    pub fn label(self) -> &'static str {
        match self {
            Fit::Gpu => "Fits GPU",
            Fit::Partial => "Partial offload",
            Fit::TooBig => "Too big",
        }
    }
}

/// Memory a model needs: weights, plus the KV cache for `ctx` tokens, plus a
/// fixed runtime overhead. The KV estimate scales with model size, which is
/// close enough for grouped-query models at fp16.
pub fn needed(file_size: u64, ctx: u64) -> u64 {
    let gb = file_size as f64 / 1e9;
    let kv = ctx as f64 * 26_000.0 * gb.max(0.5);
    file_size + kv as u64 + 400_000_000
}

pub fn fit(file_size: u64, ctx: u64, mem: Memory) -> Fit {
    let need = needed(file_size, ctx);
    if mem.vram > 0 && need as f64 <= mem.vram as f64 * 0.95 {
        Fit::Gpu
    } else if need as f64 <= mem.vram as f64 + mem.ram as f64 * 0.6 {
        Fit::Partial
    } else {
        Fit::TooBig
    }
}

/// The quant to suggest: the largest common one that fits on the GPU, else
/// the best that fits at all. Returns an index into `files`.
pub fn recommend(files: &[(String, u64)], ctx: u64, mem: Memory) -> Option<usize> {
    const GOOD: [&str; 6] = ["Q8_0", "Q6_K", "Q5_K_M", "Q4_K_M", "IQ4_XS", "Q3_K_M"];
    let pick =
        |want: Fit| GOOD.iter().find_map(|q| files.iter().position(|(quant, size)| quant == q && fit(*size, ctx, mem) == want));
    pick(Fit::Gpu).or_else(|| pick(Fit::Partial))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1_000_000_000;

    #[test]
    fn fits() {
        let mem = Memory { vram: 8 * GB, ram: 32 * GB };
        assert_eq!(fit(4_900_000_000, 8192, mem), Fit::Gpu);
        assert_eq!(fit(16 * GB, 8192, mem), Fit::Partial);
        assert_eq!(fit(40 * GB, 8192, mem), Fit::TooBig);
        let cpu_only = Memory { vram: 0, ram: 16 * GB };
        assert_eq!(fit(4 * GB, 4096, cpu_only), Fit::Partial);
    }

    #[test]
    fn recommends_biggest_that_fits() {
        let mem = Memory { vram: 8 * GB, ram: 32 * GB };
        let files = vec![
            ("Q4_K_M".to_string(), 4_900_000_000),
            ("Q5_K_M".to_string(), 5_800_000_000),
            ("Q8_0".to_string(), 8_700_000_000),
        ];
        assert_eq!(recommend(&files, 4096, mem), Some(1));
    }
}
