//! Intent classification (SPEC.md §2.3): what is the user asking for?
//!
//! Heuristic only, like the Permission Guard: no model, no I/O, so `apc` works on a fresh clone.
//! It picks the skill loaded into the system prompt; the `open-jev` sidecar may take over later.

/// What the user is asking for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Write or change code.
    GenerateCode,
    /// Explain code that already exists.
    ExplainCode,
    /// Review code and point out problems.
    ReviewCode,
    /// Anything else.
    Other,
}

impl Intent {
    /// File stem of the matching definition under `skills/`, if the intent has one.
    pub fn skill(self) -> Option<&'static str> {
        match self {
            Intent::GenerateCode => Some("generate_code"),
            Intent::ExplainCode => Some("explain_code"),
            Intent::ReviewCode => Some("review_code"),
            Intent::Other => None,
        }
    }
}

/// `(keyword, intent)`, matched case-insensitively; the keyword appearing **earliest** wins, so
/// "ajoute un commentaire" is code generation rather than an explanation. Stems on purpose.
const KEYWORDS: &[(&str, Intent)] = &[
    ("review", Intent::ReviewCode),
    ("relis", Intent::ReviewCode),
    ("relire", Intent::ReviewCode),
    ("relecture", Intent::ReviewCode),
    ("revue", Intent::ReviewCode),
    ("revois", Intent::ReviewCode),
    ("audit", Intent::ReviewCode),
    ("critique", Intent::ReviewCode),
    ("vérifi", Intent::ReviewCode),
    ("verifi", Intent::ReviewCode),
    (" bug", Intent::ReviewCode),
    ("expliqu", Intent::ExplainCode),
    ("explain", Intent::ExplainCode),
    ("pourquoi", Intent::ExplainCode),
    ("comprend", Intent::ExplainCode),
    ("understand", Intent::ExplainCode),
    ("why", Intent::ExplainCode),
    ("how does", Intent::ExplainCode),
    ("what does", Intent::ExplainCode),
    ("que fait", Intent::ExplainCode),
    ("écri", Intent::GenerateCode),
    ("ecri", Intent::GenerateCode),
    ("implément", Intent::GenerateCode),
    ("implement", Intent::GenerateCode),
    ("ajout", Intent::GenerateCode),
    ("crée", Intent::GenerateCode),
    ("cree", Intent::GenerateCode),
    ("create", Intent::GenerateCode),
    ("génèr", Intent::GenerateCode),
    ("gener", Intent::GenerateCode),
    ("refactor", Intent::GenerateCode),
    ("corrig", Intent::GenerateCode),
    (" add ", Intent::GenerateCode),
    (" fix ", Intent::GenerateCode),
    ("write", Intent::GenerateCode),
];

/// Classify `text`, falling back to [`Intent::Other`] when no keyword matches.
pub fn classify(text: &str) -> Intent {
    let text = text.to_lowercase();
    KEYWORDS
        .iter()
        .filter_map(|(keyword, intent)| text.find(keyword).map(|at| (at, *intent)))
        .min_by_key(|(at, _)| *at)
        .map_or(Intent::Other, |(_, intent)| intent)
}

#[cfg(test)]
mod tests {
    use super::{classify, Intent};

    #[test]
    fn classifies_the_four_intents() {
        assert_eq!(
            classify("écris une fonction qui trie"),
            Intent::GenerateCode
        );
        assert_eq!(
            classify("explique ce que fait main.rs"),
            Intent::ExplainCode
        );
        assert_eq!(classify("review this diff"), Intent::ReviewCode);
        assert_eq!(classify("quelle heure est-il"), Intent::Other);
    }

    #[test]
    fn the_earliest_keyword_wins() {
        // "comment" appears in "commentaire", but "ajoute" comes first: this is code generation.
        assert_eq!(
            classify("ajoute un commentaire en haut"),
            Intent::GenerateCode
        );
        assert_eq!(classify("corrige le bug de parsing"), Intent::GenerateCode);
    }

    #[test]
    fn only_the_code_intents_have_a_skill() {
        assert_eq!(Intent::GenerateCode.skill(), Some("generate_code"));
        assert_eq!(Intent::Other.skill(), None);
    }
}
