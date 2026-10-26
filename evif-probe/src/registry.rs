//! The set of families and judges a binary knows. The public `evif` binary
//! uses [`Registry::builtin`]; a companion crate can add families and judges
//! that depend on code it cannot publish and reuse the whole CLI.

use crate::family::{self, Family};
use crate::judge::{self, Judge};

use crate::rendition::{self, RenditionCodec};
use evif::RenditionFormat;

type RenditionCtor = Box<dyn Fn() -> Box<dyn RenditionCodec>>;
type FamilyCtor = Box<dyn Fn() -> Box<dyn Family>>;
type JudgeCtor = Box<dyn Fn() -> Box<dyn Judge>>;

#[derive(Default)]
pub struct Registry {
    renditions: Vec<(RenditionFormat, RenditionCtor)>,
    families: Vec<(String, FamilyCtor)>,
    judges: Vec<(String, JudgeCtor)>,
}

impl Registry {
    /// Everything in this crate.
    pub fn builtin() -> Self {
        let mut r = Self::default();
        #[cfg(feature = "possession-svt")]
        r.add_family("zenav1-svt", || Box::new(family::zenav1_svt::Zenav1Svt));
        #[cfg(feature = "possession-jxl")]
        r.add_family("zenjxl", || Box::new(family::zenjxl::Zenjxl));
        r.add_rendition_codec(RenditionFormat::Png, || Box::new(rendition::Png));
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

    /// Register a reference-decode codec without adding a candidate family.
    pub fn add_rendition_codec(
        &mut self,
        format: RenditionFormat,
        ctor: impl Fn() -> Box<dyn RenditionCodec> + 'static,
    ) {
        self.renditions.retain(|(f, _)| *f != format);
        self.renditions.push((format, Box::new(ctor)));
    }

    pub fn rendition_codec(&self, format: RenditionFormat) -> Option<Box<dyn RenditionCodec>> {
        self.renditions
            .iter()
            .find(|(f, _)| *f == format)
            .map(|(_, ctor)| ctor())
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
