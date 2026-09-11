/// Summary of what a conversion changed.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct Report {
    /// Number of pages in the document.
    pub pages: usize,
    /// Content streams (pages, forms, patterns, glyphs) that were rewritten.
    pub content_streams: usize,
    /// Color operators that were rewritten.
    pub color_operators: usize,
    /// Images (XObjects and inline images) that were converted.
    pub images: usize,
    /// Shadings (gradients) that were converted.
    pub shadings: usize,
    /// Parts of the document that were left unchanged because they could not be converted.
    pub warnings: Vec<Warning>,
}

/// A part of the document that was left unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub message: String,
    /// How many times this warning occurred.
    pub count: usize,
}

impl Report {
    /// Whether every color in the document was converted.
    pub fn is_complete(&self) -> bool {
        self.warnings.is_empty()
    }

    pub(crate) fn warn(&mut self, message: impl Into<String>) {
        let message = message.into();
        match self.warnings.iter_mut().find(|w| w.message == message) {
            Some(existing) => existing.count += 1,
            None => self.warnings.push(Warning { message, count: 1 }),
        }
    }
}
