use evif_probe::{
    family::{Encoded, Family},
    image::Rgb8Image,
    pins,
    registry::Registry,
    rng::Rng,
};
use serde_json::{Value, json};

struct Plugin;
impl Family for Plugin {
    fn name(&self) -> &'static str {
        "injected"
    }
    fn codec(&self) -> Option<(&'static str, &'static pins::Pin)> {
        None
    }
    fn sample(&self, _: &mut Rng, _: &Rgb8Image) -> Value {
        json!({"strength": 1})
    }
    fn baseline(&self, _: f64) -> Option<Value> {
        None
    }
    fn encode(&self, params: &Value, reference: &Rgb8Image) -> Result<Encoded, String> {
        if params != &json!({"strength": 1}) {
            return Err("bad recipe".into());
        }
        let mut pixels = reference.clone();
        pixels.data[0] = pixels.data[0].wrapping_add(1);
        Ok(Encoded {
            bits: None,
            pixels,
            claimed_bytes: Some(1),
        })
    }
}

#[test]
fn injected_family_uses_the_same_registry_dispatch() {
    let mut registry = Registry::builtin();
    registry.add_family("injected", || Box::new(Plugin));
    assert!(registry.family_names().contains(&"injected"));
    let family = registry.family("injected").unwrap();
    let source = Rgb8Image::new(1, 1, vec![1, 2, 3]).unwrap();
    let recipe = family.sample(&mut Rng::new(1), &source);
    let result = family.encode(&recipe, &source).unwrap();
    assert_eq!(result.pixels.data, [2, 2, 3]);
    assert_eq!(result.bytes(), Some(1));
    assert!(family.encode(&json!({"strength": 2}), &source).is_err());
}
