//! Text engine (spec docs/spec/01-text-engine.md): fonts → metrics → parse → layout.

pub mod fonts;
pub mod metrics;
pub mod style;
pub mod tree;
pub mod whitespace;

/// User-facing warnings collected while measuring/parsing (deduplicated).
#[derive(Debug, Default)]
pub struct Warnings(pub Vec<String>);

impl Warnings {
    pub fn push(&mut self, s: impl Into<String>) {
        let s = s.into();
        if !self.0.contains(&s) {
            self.0.push(s);
        }
    }
}
