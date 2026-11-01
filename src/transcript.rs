//! The running transcript a caption shows: committed text, which never
//! changes, followed by the backend's current tentative hypothesis.

/// How much committed text to keep. The overlay shows a few lines from the
/// end, so older text is only memory.
const KEEP_CHARS: usize = 4000;

#[derive(Default, Debug)]
pub struct Transcript {
    committed: String,
    /// The open item's committed text, replaced wholesale by its
    /// `completed` transcript.
    item: String,
    item_id: String,
    tentative: String,
}

impl Transcript {
    /// A delta. Committed deltas extend the item; an unstable one replaces
    /// the hypothesis after it.
    pub fn delta(&mut self, item_id: &str, text: &str, unstable: bool) {
        self.enter(item_id);
        if unstable {
            self.tentative = text.trim().to_owned();
        } else {
            push_words(&mut self.item, text);
            self.tentative.clear();
        }
    }

    /// The item's terminal transcript, which supersedes its deltas.
    pub fn completed(&mut self, item_id: &str, transcript: &str) {
        self.enter(item_id);
        self.item = transcript.trim().to_owned();
        self.tentative.clear();
        self.close_item();
    }

    fn enter(&mut self, item_id: &str) {
        if self.item_id != item_id {
            self.close_item();
            self.item_id = item_id.to_owned();
        }
    }

    fn close_item(&mut self) {
        let item = std::mem::take(&mut self.item);
        push_words(&mut self.committed, &item);
        self.item_id.clear();
        if self.committed.len() > KEEP_CHARS {
            let cut = self.committed.len() - KEEP_CHARS;
            let cut = (cut..self.committed.len())
                .find(|&i| self.committed.is_char_boundary(i) && self.committed[i..].starts_with(' '))
                .unwrap_or(cut);
            self.committed.drain(..cut);
            self.committed = self.committed.trim_start().to_owned();
        }
    }

    /// The text to show, split where the tentative tail starts.
    pub fn display(&self) -> (String, usize) {
        let mut text = self.committed.clone();
        push_words(&mut text, &self.item);
        let stable = text.len();
        push_words(&mut text, &self.tentative);
        (text, stable)
    }
}

fn push_words(into: &mut String, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    if !into.is_empty() {
        into.push(' ');
    }
    into.push_str(text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unstable_hypotheses_replace_each_other() {
        let mut t = Transcript::default();
        t.delta("a", "We've also", true);
        t.delta("a", "We have also implemented", true);
        assert_eq!(t.display(), ("We have also implemented".into(), 0));
    }

    #[test]
    fn completed_items_stay_and_the_next_one_follows() {
        let mut t = Transcript::default();
        t.delta("a", "hello", true);
        t.completed("a", "Hello there.");
        t.delta("b", "General", true);
        assert_eq!(t.display(), ("Hello there. General".into(), 12));
    }

    #[test]
    fn committed_deltas_accumulate_before_the_tail() {
        let mut t = Transcript::default();
        t.delta("a", "One.", false);
        t.delta("a", "Two", true);
        t.delta("a", "Two.", false);
        assert_eq!(t.display(), ("One. Two.".into(), 9));
    }

    #[test]
    fn a_new_item_closes_the_open_one() {
        let mut t = Transcript::default();
        t.delta("a", "First.", false);
        t.delta("b", "Second", true);
        assert_eq!(t.display(), ("First. Second".into(), 6));
    }

    #[test]
    fn old_text_is_dropped_at_a_word() {
        let mut t = Transcript::default();
        for i in 0..2000 {
            t.completed(&i.to_string(), "word");
        }
        let (text, _) = t.display();
        assert!(text.len() <= KEEP_CHARS && text.starts_with("word"));
    }
}
