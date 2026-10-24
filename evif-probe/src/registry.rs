//! The set of families and judges a binary knows. The public `evif` binary
//! uses [`Registry::builtin`]; a companion crate can add families and judges
//! that depend on code it cannot publish and reuse the whole CLI.

use crate::family::{self, Family};
use crate::judge::{self, Judge};

type FamilyCtor = Box<dyn Fn() -> Box<dyn Family>>;
type JudgeCtor = Box<dyn Fn() -> Box<dyn Judge>>;

#[derive(Default)]
pub struct Registry {
    families: Vec<(String, FamilyCtor)>,
    judges: Vec<(String, JudgeCtor)>,
}

impl Registry {
    /// Everything in this crate.
    pub fn builtin() -> Self {
        let mut r = Self::default();
        r.add_family("zenjpeg-expert", || {
            Box::new(family::zenjpeg_expert::ZenjpegExpert)
        });
        r.add_family("zenjpeg-color", || {
            Box::new(family::zenjpeg_color::ZenjpegColor)
        });
        r.add_family("zenpng-palette", || {
            Box::new(family::zenpng_palette::ZenpngPalette)
        });
        r.add_family("corruption", || Box::new(family::corruption::Corruption));
        r.add_judge("zensim", || Box::new(judge::Zensim::new()));
        r.add_judge("ssim2", || Box::new(judge::Ssim2));
        r.add_judge("butteraugli", || Box::new(judge::Butteraugli));
        r
    }

    /// Register a family under `name`; a later registration of the same name
    /// replaces the earlier one.
    pub fn add_family(&mut self, name: &str, ctor: impl Fn() -> Box<dyn Family> + 'static) {
        self.families.retain(|(n, _)| n != name);
        self.families.push((name.to_string(), Box::new(ctor)));
    }

    pub fn add_judge(&mut self, name: &str, ctor: impl Fn() -> Box<dyn Judge> + 'static) {
        self.judges.retain(|(n, _)| n != name);
        self.judges.push((name.to_string(), Box::new(ctor)));
    }

    pub fn family(&self, name: &str) -> Option<Box<dyn Family>> {
        self.families
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, c)| c())
    }

    pub fn judge(&self, name: &str) -> Option<Box<dyn Judge>> {
        self.judges
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, c)| c())
    }

    pub fn family_names(&self) -> Vec<&str> {
        self.families.iter().map(|(n, _)| n.as_str()).collect()
    }

    pub fn judge_names(&self) -> Vec<&str> {
        self.judges.iter().map(|(n, _)| n.as_str()).collect()
    }

    /// Every judge, in registration order.
    pub fn all_judges(&self) -> Vec<Box<dyn Judge>> {
        self.judges.iter().map(|(_, c)| c()).collect()
    }
}
