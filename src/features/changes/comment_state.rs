use super::{Comment, HashMap, HashSet, Instant, comments_stamp, read_comments};

pub(super) struct CommentState {
    /// Review comments, for 1 batch message (`C`).
    pub(super) comments: Vec<Comment>,
    pub(super) comments_at: Option<std::time::SystemTime>,
    /// The comments as last read / written (the base of a save's merge).
    pub(super) comments_base: Vec<Comment>,
    /// The comments in the file view: 1 row each (C), else their whole text and replies.
    pub(super) comments_folded: bool,
    /// Comments (ids) folded the other way than `comments_folded` (enter, C on a comment).
    pub(super) comment_flip: HashSet<String>,
    /// A comment to show (a reply note's jump): once its file is read.
    pub(super) want_comment: Option<String>,
    /// Expanded comments with unseen agent replies on screen: since when (1 s: seen).
    pub(super) seen_at: HashMap<String, Instant>,
    /// The `@` dropdown of the comment modal.
    pub(super) cmention: crate::ui::mention::Mention,
}

impl CommentState {
    pub(super) fn new(id: &str) -> Self {
        let comments = read_comments(id);

        Self {
            comments_at: comments_stamp(id),
            comments_base: comments.clone(),
            comments,
            comments_folded: false,
            comment_flip: HashSet::new(),
            want_comment: None,
            seen_at: HashMap::new(),
            cmention: crate::ui::mention::Mention::default(),
        }
    }
}
