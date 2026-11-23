//! dsky-tzc: a cut from the middle of a line leaves its spaces on both sides of it in the kept
//! text: "it.  Then" in the body. The file's body takes one of them away: where the spaces on
//! both sides would meet, or a space would meet a line's end or punctuation, the stash record
//! says `"space":"after"` or `"space":"before"`, and a load puts the space back beside the cut,
//! so the page comes back exactly as it was saved and other editors read clean text.
//!
//! Between [`take`] and the writer, and between the reader's planting and [`restore`], the space a
//! cut took rides in its content as a marker char that never leaves this crate.

use doc_model::snapshot::{DocTree, TreeNode};

const BEFORE: char = '\u{F8FE}';
const AFTER: char = '\u{F8FF}';

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    Before,
    After,
}

impl Side {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Side::Before => "before",
            Side::After => "after",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Side> {
        match s {
            "before" => Some(Side::Before),
            "after" => Some(Side::After),
            _ => None,
        }
    }
}

/// The tree as it is written: a lone space beside a cut leaves the kept text, marked in the cut.
pub(crate) fn take(mut tree: DocTree) -> DocTree {
    take_in(&mut tree.page, true);
    for n in &mut tree.notes {
        take_in(&mut n.content, true);
    }
    tree
}

/// The cut's content without the marker, and the side its space came from.
pub(crate) fn strip(content: &[TreeNode]) -> (Vec<TreeNode>, Option<Side>) {
    let marker = |n: &TreeNode, c: char| matches!(n, TreeNode::Text { text } if text.chars().eq([c]));
    let side = if content.iter().any(|n| marker(n, BEFORE)) {
        Some(Side::Before)
    } else if content.iter().any(|n| marker(n, AFTER)) {
        Some(Side::After)
    } else {
        return (content.to_vec(), None);
    };
    (content.iter().filter(|n| !marker(n, BEFORE) && !marker(n, AFTER)).cloned().collect(), side)
}

/// Marks a cut's content, as planted by the reader, with the side its space goes back on.
pub(crate) fn mark(content: &mut Vec<TreeNode>, side: Side) {
    match side {
        Side::Before => content.insert(0, TreeNode::text(BEFORE.to_string())),
        Side::After => content.push(TreeNode::text(AFTER.to_string())),
    }
}

/// After every cut is planted: each marked cut gets its space back, in the text it came from,
/// unless the text there (edited elsewhere) already has one.
pub(crate) fn restore(nodes: &mut Vec<TreeNode>) {
    let mut i = 0;
    while i < nodes.len() {
        match &mut nodes[i] {
            TreeNode::Group(g) => {
                for v in &mut g.variants {
                    restore(&mut v.content);
                }
            }
            TreeNode::Cut(c) => {
                let (content, side) = strip(&c.content);
                if let Some(side) = side {
                    c.content = content;
                    let found = match side {
                        Side::After => (i + 1..nodes.len()).take_while(|&k| passes(&nodes[k]) || is_text(&nodes[k])).find(|&k| is_text(&nodes[k])),
                        Side::Before => (0..i).rev().take_while(|&k| passes(&nodes[k]) || is_text(&nodes[k])).find(|&k| is_text(&nodes[k])),
                    };
                    match found.map(|k| &mut nodes[k]) {
                        Some(TreeNode::Text { text }) => match side {
                            Side::After if !text.starts_with(char::is_whitespace) => text.insert(0, ' '),
                            Side::Before if !text.ends_with(char::is_whitespace) => text.push(' '),
                            _ => {}
                        },
                        _ => {
                            nodes.insert(if side == Side::After { i + 1 } else { i }, TreeNode::text(" "));
                            i += 1;
                        }
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
}

fn is_text(n: &TreeNode) -> bool {
    matches!(n, TreeNode::Text { text } if !text.is_empty())
}

/// Takes no room between a cut and the text beside it.
fn passes(n: &TreeNode) -> bool {
    matches!(n, TreeNode::Cut(_) | TreeNode::CommentStart(_) | TreeNode::CommentEnd { .. })
        || matches!(n, TreeNode::Text { text } if text.is_empty())
}

/// `top`: the page or a note, whose start is a line's start; a version's edges aren't.
fn take_in(out: &mut Vec<TreeNode>, top: bool) {
    for n in out.iter_mut() {
        if let TreeNode::Group(g) = n {
            for v in &mut g.variants {
                take_in(&mut v.content, false);
            }
        }
    }
    // A move can change what an earlier cut sees: again until nothing moves (each move leaves
    // one kept space fewer), so a second save finds nothing more to take.
    let mut moved = true;
    while moved {
        moved = false;
        for i in 0..out.len() {
            match &out[i] {
                TreeNode::Cut(c) if !c.paragraph && strip(&c.content).1.is_none() => {}
                _ => continue,
            }
            // The kept chars on each side, nearest first, across text nodes (a load may split
            // them otherwise), the nearest text node, and whether that side ends the container.
            let side = |ks: &mut dyn Iterator<Item = usize>, from_end: bool| {
                let (mut cs, mut first, mut edge) = (Vec::new(), None, true);
                for k in ks {
                    match &out[k] {
                        TreeNode::Text { text } if !text.is_empty() => {
                            first.get_or_insert(k);
                            if from_end {
                                cs.extend(text.chars().rev());
                            } else {
                                cs.extend(text.chars());
                            }
                            if cs.len() >= 2 {
                                edge = false;
                                break;
                            }
                        }
                        n if passes(n) => {}
                        _ => {
                            edge = false;
                            break;
                        }
                    }
                }
                (cs, first, edge)
            };
            let (back, prev, page_start) = side(&mut (0..i).rev(), true);
            let (fwd, next, page_end) = side(&mut (i + 1..out.len()), false);
            let before = back.first().copied().or((top && page_start).then_some('\n'));
            // So do its start and end (a saved body ends in a newline).
            let after = fwd.first().copied().or((top && page_end).then_some('\n'));
            // Only a lone space goes, so the next save finds nothing more to take.
            let lone = |cs: &[char]| cs.len() >= 2 && cs[0] == ' ' && !cs[1].is_whitespace();
            let take_after = lone(&fwd) && matches!(before, Some(' ' | '\n'));
            // At a line's end only after prose: "# " or "- " there is Markdown ("-" under a
            // paragraph would make it a heading).
            let prose = |c: char| c.is_alphanumeric() || ".,;:!?)]}”’\"'…".contains(c);
            let take_before = !take_after
                && lone(&back)
                && match after {
                    Some('\n') => prose(back[1]),
                    Some(c) => ".,;:!?)]}”’".contains(c),
                    None => false,
                };
            let (side, from) = match (take_after, take_before) {
                (true, _) => (Side::After, next),
                (_, true) => (Side::Before, prev),
                _ => continue,
            };
            // The text it comes from keeps a char: two cuts with only the space between them
            // would load as one spot, and the space couldn't go back between them.
            let Some(from) = from.filter(|&k| matches!(&out[k], TreeNode::Text { text } if text.chars().nth(1).is_some())) else { continue };
            if let TreeNode::Text { text } = &mut out[from] {
                match side {
                    Side::After => {
                        text.remove(0);
                    }
                    Side::Before => {
                        text.pop();
                    }
                }
            }
            if let TreeNode::Cut(c) = &mut out[i] {
                mark(&mut c.content, side);
            }
            moved = true;
        }
    }
    out.retain(|n| !matches!(n, TreeNode::Text { text } if text.is_empty()));
}
