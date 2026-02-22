use ratatui::prelude::Text;

pub struct LineHistory {
    pub text: Text<'static>,
}

impl LineHistory {
    const MAX_HISTORY: usize = 1000;

    pub fn new() -> Self {
        Self {
            text: Text::default(),
        }
    }

    pub fn push(&mut self, text: Text<'static>) {
        self.text.extend(text);
        self.text
            .lines
            .drain(0..self.text.lines.len().saturating_sub(Self::MAX_HISTORY));
    }
}
