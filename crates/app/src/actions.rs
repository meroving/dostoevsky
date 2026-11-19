use gpui_kit::*;
use l10n::{tr, trc};

use crate::prefs::{Alignment, Appearance, ParagraphLook, Prefs};

#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = app, no_json)]
pub struct SelectTheme {
    pub id: String,
}

actions!(
    editor,
    [
        MoveLeft,
        MoveRight,
        MoveUp,
        MoveDown,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        MoveWordLeft,
        MoveWordRight,
        SelectWordLeft,
        SelectWordRight,
        MoveLineStart,
        MoveLineEnd,
        SelectLineStart,
        SelectLineEnd,
        MoveDocStart,
        MoveDocEnd,
        SelectDocStart,
        SelectDocEnd,
        MoveParagraphStart,
        MoveParagraphEnd,
        SelectParagraphStart,
        SelectParagraphEnd,
        PageUp,
        PageDown,
        SelectPageUp,
        SelectPageDown,
        Backspace,
        Delete,
        DeleteWordLeft,
        DeleteWordRight,
        DeleteLineLeft,
        DeleteLineRight,
        DeleteToParagraphEnd,
        Transpose,
        Newline,
        LineBreak,
        Tab,
        Backtab,
        SelectAll,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
        Bold,
        Italic,
        ShowCharacterPalette,
        Cancel,
        Confirm,
        PrevSentence,
        NextSentence,
        SelectPrevSentence,
        SelectNextSentence,
        GrowSelection,
        ShrinkSelection,
    ]
);

actions!(
    page,
    [
        MoveUnitUp,
        MoveUnitDown,
        CycleNext,
        CyclePrev,
        CycleAtCaretNext,
        CycleAtCaretPrev,
        BackToOriginal,
        NextGroup,
        PrevGroup,
        AlternativesForSelection,
        AiAlternatives,
        SettleOnThis,
        ToggleLayer,
        WritingMode,
        EditingMode,
        GhostToggle,
        RemoveAllGhosted,
        StashToOverflow,
        ToggleOverflow,
        InsertOverflowNote,
        ViewSource,
        // The old name stays, so keymaps that bind it keep working.
        CopyFinalMarkdown,
        Export,
        Print,
        ShowShortcuts,
        ShowWelcome,
        AllAlternatives,
        SidebarPrevGroup,
        SidebarNextGroup,
        Find,
        FindReplace,
        FindNext,
        FindPrevious,
        UseSelectionForFind,
        FindInProject,
        ToggleLab,
        NextMark,
        PrevMark,
        AddComment,
        DeleteComment,
        DeleteAllComments,
    ]
);

actions!(
    lab,
    [
        LabMenuUp, LabMenuDown, LabMenuLeft, LabMenuRight, LabMenuRun, LabMenu1, LabMenu2, LabMenu3, LabMenu4, LabEscape, TrimLessFine,
        TrimMoreFine, ReviewSecondary, ReviewKeep, ReviewComment
    ]
);

actions!(
    app,
    [
        NewDocument,
        Open,
        OpenRecent,
        Save,
        SaveAs,
        RevertToSaved,
        CloseWindow,
        Quit,
        Hide,
        HideOthers,
        ShowAll,
        ShowPreferences,
        CreateThemeSheet,
        About,
        CheckForUpdates,
        Minimize,
        Zoom,
        ToggleFullScreen,
        ToggleZenWriting,
        ToggleTypewriter,
        TogglePrivate,
        AlignLeft,
        AlignJustified,
        ToggleHyphenation,
        TextBigger,
        TextSmaller,
        TextDefaultSize,
        ParagraphsSpaced,
        ParagraphsIndented,
        AppearanceDark,
        AppearanceLight,
        AppearanceSystem,
        ToggleAppearance,
        OpenKeymapFile,
        NewLooseDocument,
        NewProject,
        ProjectSettings,
        OpenEnclosingFolder,
        ToggleProject,
        PreviousFile,
        NextFile,
    ]
);

actions!(alternatives, [AltListUp, AltListDown, AltListOpen, AltListBack]);

actions!(project, [ListUp, ListDown, ListReturn, ListClose, ListTrash]);

actions!(find, [ToggleMatchCase, ToggleWholeWords]);

/// Title and (label, action) rows.
pub type ShortcutGroup = (&'static str, Vec<(&'static str, Box<dyn Action>)>);

/// (keys, context, action) for every default binding.
pub fn default_bindings_spec() -> Vec<(&'static str, Option<&'static str>, Box<dyn Action>)> {
    let t = Some("TextView");
    let p = Some("Page");
    vec![
        ("left", t, Box::new(MoveLeft) as Box<dyn Action>),
        ("right", t, Box::new(MoveRight) as Box<dyn Action>),
        ("up", t, Box::new(MoveUp) as Box<dyn Action>),
        ("down", t, Box::new(MoveDown) as Box<dyn Action>),
        ("shift-left", t, Box::new(SelectLeft) as Box<dyn Action>),
        ("shift-right", t, Box::new(SelectRight) as Box<dyn Action>),
        ("shift-up", t, Box::new(SelectUp) as Box<dyn Action>),
        ("shift-down", t, Box::new(SelectDown) as Box<dyn Action>),
        ("alt-left", t, Box::new(MoveWordLeft) as Box<dyn Action>),
        ("alt-right", t, Box::new(MoveWordRight) as Box<dyn Action>),
        ("alt-shift-left", t, Box::new(SelectWordLeft) as Box<dyn Action>),
        ("alt-shift-right", t, Box::new(SelectWordRight) as Box<dyn Action>),
        ("cmd-left", t, Box::new(MoveLineStart) as Box<dyn Action>),
        ("cmd-right", t, Box::new(MoveLineEnd) as Box<dyn Action>),
        ("home", t, Box::new(MoveLineStart) as Box<dyn Action>),
        ("end", t, Box::new(MoveLineEnd) as Box<dyn Action>),
        ("ctrl-a", t, Box::new(MoveLineStart) as Box<dyn Action>),
        ("ctrl-e", t, Box::new(MoveLineEnd) as Box<dyn Action>),
        ("cmd-shift-left", t, Box::new(SelectLineStart) as Box<dyn Action>),
        ("cmd-shift-right", t, Box::new(SelectLineEnd) as Box<dyn Action>),
        ("cmd-up", t, Box::new(MoveDocStart) as Box<dyn Action>),
        ("cmd-down", t, Box::new(MoveDocEnd) as Box<dyn Action>),
        ("cmd-shift-up", t, Box::new(SelectDocStart) as Box<dyn Action>),
        ("cmd-shift-down", t, Box::new(SelectDocEnd) as Box<dyn Action>),
        ("shift-home", t, Box::new(SelectLineStart) as Box<dyn Action>),
        ("shift-end", t, Box::new(SelectLineEnd) as Box<dyn Action>),
        ("alt-up", t, Box::new(MoveParagraphStart) as Box<dyn Action>),
        ("alt-down", t, Box::new(MoveParagraphEnd) as Box<dyn Action>),
        ("alt-shift-up", t, Box::new(SelectParagraphStart) as Box<dyn Action>),
        ("alt-shift-down", t, Box::new(SelectParagraphEnd) as Box<dyn Action>),
        // Moving a sentence comes after ⌥↑ ⌥↓ above, so it wins while whole sentences or
        // paragraphs are selected in editing.
        ("ctrl-cmd-left", t, Box::new(PrevSentence) as Box<dyn Action>),
        ("ctrl-cmd-right", t, Box::new(NextSentence) as Box<dyn Action>),
        ("ctrl-shift-cmd-left", t, Box::new(SelectPrevSentence) as Box<dyn Action>),
        ("ctrl-shift-cmd-right", t, Box::new(SelectNextSentence) as Box<dyn Action>),
        ("ctrl-cmd-up", t, Box::new(GrowSelection) as Box<dyn Action>),
        ("ctrl-cmd-down", t, Box::new(ShrinkSelection) as Box<dyn Action>),
        ("alt-up", Some("Page && whole_unit"), Box::new(MoveUnitUp) as Box<dyn Action>),
        ("alt-down", Some("Page && whole_unit"), Box::new(MoveUnitDown) as Box<dyn Action>),
        ("pageup", t, Box::new(PageUp) as Box<dyn Action>),
        ("pagedown", t, Box::new(PageDown) as Box<dyn Action>),
        ("shift-pageup", t, Box::new(SelectPageUp) as Box<dyn Action>),
        ("shift-pagedown", t, Box::new(SelectPageDown) as Box<dyn Action>),
        ("ctrl-f", t, Box::new(MoveRight) as Box<dyn Action>),
        ("ctrl-b", t, Box::new(MoveLeft) as Box<dyn Action>),
        ("ctrl-n", t, Box::new(MoveDown) as Box<dyn Action>),
        ("ctrl-p", t, Box::new(MoveUp) as Box<dyn Action>),
        ("ctrl-h", t, Box::new(Backspace) as Box<dyn Action>),
        ("ctrl-k", t, Box::new(DeleteToParagraphEnd) as Box<dyn Action>),
        ("ctrl-t", t, Box::new(Transpose) as Box<dyn Action>),
        ("backspace", t, Box::new(Backspace) as Box<dyn Action>),
        ("shift-backspace", t, Box::new(Backspace) as Box<dyn Action>),
        ("delete", t, Box::new(Delete) as Box<dyn Action>),
        ("ctrl-d", t, Box::new(Delete) as Box<dyn Action>),
        ("alt-backspace", t, Box::new(DeleteWordLeft) as Box<dyn Action>),
        ("alt-delete", t, Box::new(DeleteWordRight) as Box<dyn Action>),
        ("cmd-backspace", t, Box::new(DeleteLineLeft) as Box<dyn Action>),
        ("cmd-delete", t, Box::new(DeleteLineRight) as Box<dyn Action>),
        ("enter", t, Box::new(Newline) as Box<dyn Action>),
        ("shift-enter", t, Box::new(LineBreak) as Box<dyn Action>),
        ("tab", t, Box::new(Tab) as Box<dyn Action>),
        ("shift-tab", t, Box::new(Backtab) as Box<dyn Action>),
        ("cmd-a", t, Box::new(SelectAll) as Box<dyn Action>),
        ("cmd-c", t, Box::new(Copy) as Box<dyn Action>),
        ("cmd-x", t, Box::new(Cut) as Box<dyn Action>),
        ("cmd-v", t, Box::new(Paste) as Box<dyn Action>),
        ("cmd-z", t, Box::new(Undo) as Box<dyn Action>),
        ("cmd-shift-z", t, Box::new(Redo) as Box<dyn Action>),
        ("cmd-b", p, Box::new(Bold) as Box<dyn Action>),
        ("cmd-i", p, Box::new(Italic) as Box<dyn Action>),
        ("ctrl-cmd-space", t, Box::new(ShowCharacterPalette) as Box<dyn Action>),
        ("escape", t, Box::new(Cancel) as Box<dyn Action>),
        ("escape", Some("Sheet"), Box::new(Cancel) as Box<dyn Action>),
        ("escape", Some("About"), Box::new(Cancel) as Box<dyn Action>),
        ("enter", Some("TextView && Field"), Box::new(Confirm) as Box<dyn Action>),
        // The bare arrows cycle only over a group with the layer on; later bindings win, so
        // these come after the caret ones.
        ("down", Some("Page && hovering_group"), Box::new(CycleNext) as Box<dyn Action>),
        ("up", Some("Page && hovering_group"), Box::new(CyclePrev) as Box<dyn Action>),
        ("alt-cmd-down", p, Box::new(CycleAtCaretNext) as Box<dyn Action>),
        ("alt-cmd-up", p, Box::new(CycleAtCaretPrev) as Box<dyn Action>),
        ("alt-cmd-right", p, Box::new(NextGroup) as Box<dyn Action>),
        ("alt-cmd-left", p, Box::new(PrevGroup) as Box<dyn Action>),
        ("alt-cmd-0", p, Box::new(BackToOriginal) as Box<dyn Action>),
        ("cmd-shift-a", None, Box::new(AlternativesForSelection) as Box<dyn Action>),
        ("alt-cmd-a", None, Box::new(AiAlternatives) as Box<dyn Action>),
        ("alt-cmd-enter", None, Box::new(SettleOnThis) as Box<dyn Action>),
        // The numbers are the mode button's segments, left to right; ⌘D flips between them.
        ("cmd-1", None, Box::new(WritingMode) as Box<dyn Action>),
        ("cmd-2", None, Box::new(EditingMode) as Box<dyn Action>),
        ("cmd-d", None, Box::new(ToggleLayer) as Box<dyn Action>),
        ("cmd-/", p, Box::new(GhostToggle) as Box<dyn Action>),
        ("cmd-shift-x", p, Box::new(StashToOverflow) as Box<dyn Action>),
        ("ctrl-cmd-c", None, Box::new(ToggleOverflow) as Box<dyn Action>),
        ("cmd-enter", Some("Note"), Box::new(InsertOverflowNote) as Box<dyn Action>),
        ("cmd-z", Some("Note"), Box::new(Undo) as Box<dyn Action>),
        ("cmd-shift-z", Some("Note"), Box::new(Redo) as Box<dyn Action>),
        ("escape", Some("Note"), Box::new(Cancel) as Box<dyn Action>),
        ("alt-cmd-u", None, Box::new(ViewSource) as Box<dyn Action>),
        ("cmd-shift-c", None, Box::new(CopyFinalMarkdown) as Box<dyn Action>),
        ("cmd-shift-e", None, Box::new(Export) as Box<dyn Action>),
        ("alt-cmd-p", None, Box::new(Print) as Box<dyn Action>),
        ("enter", Some("Export"), Box::new(Confirm) as Box<dyn Action>),
        ("cmd-shift-k", None, Box::new(ShowShortcuts) as Box<dyn Action>),
        ("alt-cmd-m", None, Box::new(AddComment) as Box<dyn Action>),
        ("cmd-backspace", Some("TextView && Comment"), Box::new(DeleteComment) as Box<dyn Action>),
        // While the alternatives panel is open these win over the text's ⌥↑ / ⌥↓.
        ("alt-up", Some("Workspace && sidebar"), Box::new(SidebarPrevGroup) as Box<dyn Action>),
        ("alt-down", Some("Workspace && sidebar"), Box::new(SidebarNextGroup) as Box<dyn Action>),
        ("alt-up", Some("Workspace && sidebar > TextView"), Box::new(SidebarPrevGroup) as Box<dyn Action>),
        ("alt-down", Some("Workspace && sidebar > TextView"), Box::new(SidebarNextGroup) as Box<dyn Action>),
        ("up", Some("AltList"), Box::new(AltListUp) as Box<dyn Action>),
        ("down", Some("AltList"), Box::new(AltListDown) as Box<dyn Action>),
        ("enter", Some("AltList"), Box::new(AltListOpen) as Box<dyn Action>),
        ("right", Some("AltList"), Box::new(AltListOpen) as Box<dyn Action>),
        ("escape", Some("AltList"), Box::new(AltListBack) as Box<dyn Action>),
        ("cmd-z", Some("AltList"), Box::new(Undo) as Box<dyn Action>),
        ("cmd-shift-z", Some("AltList"), Box::new(Redo) as Box<dyn Action>),
        ("alt-cmd-l", None, Box::new(ToggleLab) as Box<dyn Action>),
        ("alt-cmd-]", None, Box::new(NextMark) as Box<dyn Action>),
        ("alt-cmd-[", None, Box::new(PrevMark) as Box<dyn Action>),
        ("up", Some("Review"), Box::new(LabMenuUp) as Box<dyn Action>),
        ("down", Some("Review"), Box::new(LabMenuDown) as Box<dyn Action>),
        ("left", Some("Review"), Box::new(LabMenuLeft) as Box<dyn Action>),
        ("right", Some("Review"), Box::new(LabMenuRight) as Box<dyn Action>),
        ("alt-left", Some("Review"), Box::new(TrimLessFine) as Box<dyn Action>),
        ("alt-right", Some("Review"), Box::new(TrimMoreFine) as Box<dyn Action>),
        ("enter", Some("Review"), Box::new(LabMenuRun) as Box<dyn Action>),
        ("backspace", Some("Review"), Box::new(ReviewSecondary) as Box<dyn Action>),
        ("k", Some("Review"), Box::new(ReviewKeep) as Box<dyn Action>),
        ("c", Some("Review"), Box::new(ReviewComment) as Box<dyn Action>),
        ("1", Some("Review"), Box::new(LabMenu1) as Box<dyn Action>),
        ("2", Some("Review"), Box::new(LabMenu2) as Box<dyn Action>),
        ("3", Some("Review"), Box::new(LabMenu3) as Box<dyn Action>),
        ("4", Some("Review"), Box::new(LabMenu4) as Box<dyn Action>),
        ("escape", Some("Review"), Box::new(LabEscape) as Box<dyn Action>),
        ("cmd-z", Some("Review"), Box::new(Undo) as Box<dyn Action>),
        ("cmd-shift-z", Some("Review"), Box::new(Redo) as Box<dyn Action>),
        ("cmd-f", None, Box::new(Find) as Box<dyn Action>),
        ("alt-cmd-f", None, Box::new(FindReplace) as Box<dyn Action>),
        ("cmd-g", None, Box::new(FindNext) as Box<dyn Action>),
        ("cmd-shift-g", None, Box::new(FindPrevious) as Box<dyn Action>),
        ("cmd-e", None, Box::new(UseSelectionForFind) as Box<dyn Action>),
        ("shift-enter", Some("FindBar > TextView"), Box::new(FindPrevious) as Box<dyn Action>),
        ("cmd-shift-f", None, Box::new(FindInProject) as Box<dyn Action>),
        ("alt-cmd-c", Some("FindBar"), Box::new(ToggleMatchCase) as Box<dyn Action>),
        ("alt-cmd-w", Some("FindBar"), Box::new(ToggleWholeWords) as Box<dyn Action>),
        ("cmd-n", None, Box::new(NewDocument) as Box<dyn Action>),
        ("cmd-shift-n", None, Box::new(NewLooseDocument) as Box<dyn Action>),
        ("alt-cmd-n", None, Box::new(NewProject) as Box<dyn Action>),
        ("cmd-o", None, Box::new(Open) as Box<dyn Action>),
        ("cmd-shift-t", None, Box::new(OpenRecent) as Box<dyn Action>),
        ("cmd-s", None, Box::new(Save) as Box<dyn Action>),
        ("cmd-shift-s", None, Box::new(SaveAs) as Box<dyn Action>),
        ("cmd-w", None, Box::new(CloseWindow) as Box<dyn Action>),
        ("cmd-q", None, Box::new(Quit) as Box<dyn Action>),
        ("cmd-h", None, Box::new(Hide) as Box<dyn Action>),
        ("alt-cmd-h", None, Box::new(HideOthers) as Box<dyn Action>),
        ("cmd-,", None, Box::new(ShowPreferences) as Box<dyn Action>),
        ("cmd-m", None, Box::new(Minimize) as Box<dyn Action>),
        ("ctrl-cmd-f", None, Box::new(ToggleFullScreen) as Box<dyn Action>),
        ("ctrl-cmd-z", None, Box::new(ToggleZenWriting) as Box<dyn Action>),
        ("ctrl-cmd-t", None, Box::new(ToggleTypewriter) as Box<dyn Action>),
        // ⌘H stays Hide.
        ("cmd-shift-h", None, Box::new(TogglePrivate) as Box<dyn Action>),
        ("ctrl-cmd-l", None, Box::new(ToggleAppearance) as Box<dyn Action>),
        // ⌘+ is how macOS reports ⌘⇧=; listed first, so the menu bar shows ⌘+ (as Pages and
        // Safari do) and the shortcuts sheet's last-wins shows ⌘=.
        ("cmd-+", None, Box::new(TextBigger) as Box<dyn Action>),
        ("cmd-=", None, Box::new(TextBigger) as Box<dyn Action>),
        ("cmd--", None, Box::new(TextSmaller) as Box<dyn Action>),
        ("cmd-0", None, Box::new(TextDefaultSize) as Box<dyn Action>),
        ("cmd-p", None, Box::new(ToggleProject) as Box<dyn Action>),
        ("cmd-[", None, Box::new(PreviousFile) as Box<dyn Action>),
        ("cmd-]", None, Box::new(NextFile) as Box<dyn Action>),
        ("up", Some("ProjectList"), Box::new(ListUp) as Box<dyn Action>),
        ("down", Some("ProjectList"), Box::new(ListDown) as Box<dyn Action>),
        ("enter", Some("ProjectList"), Box::new(ListReturn) as Box<dyn Action>),
        ("escape", Some("ProjectList"), Box::new(ListClose) as Box<dyn Action>),
        ("cmd-backspace", Some("ProjectList"), Box::new(ListTrash) as Box<dyn Action>),
        ("cmd-z", Some("ProjectList"), Box::new(Undo) as Box<dyn Action>),
    ]
}

/// (title, [(label, action)]); keys are looked up live.
pub fn shortcut_groups() -> Vec<ShortcutGroup> {
    vec![
        (
            tr!("Alternatives"),
            vec![
                (tr!("Cycle under pointer"), Box::new(CycleNext) as Box<dyn Action>),
                (tr!("Cycle at caret"), Box::new(CycleAtCaretNext)),
                (tr!("Next group"), Box::new(NextGroup)),
                (tr!("Previous group"), Box::new(PrevGroup)),
                (tr!("Back to original"), Box::new(BackToOriginal)),
                (tr!("Alternatives for selection"), Box::new(AlternativesForSelection)),
                (tr!("AI alternatives"), Box::new(AiAlternatives)),
                (tr!("Settle on this"), Box::new(SettleOnThis)),
                (tr!("Writing mode"), Box::new(WritingMode)),
                (tr!("Editing mode"), Box::new(EditingMode)),
                (tr!("Switch writing / editing"), Box::new(ToggleLayer)),
            ],
        ),
        (
            tr!("Alternatives panel"),
            vec![
                (tr!("Previous group"), Box::new(SidebarPrevGroup) as Box<dyn Action>),
                (tr!("Next group"), Box::new(SidebarNextGroup)),
                (tr!("A remembered steer, after ??"), Box::new(Tab)),
                (tr!("Back / close"), Box::new(Cancel)),
            ],
        ),
        (
            l10n::trc!("pass menu", "Review"),
            vec![
                (tr!("Review tab"), Box::new(ToggleLab) as Box<dyn Action>),
                (tr!("Next mark"), Box::new(NextMark)),
                (tr!("Previous mark"), Box::new(PrevMark)),
            ],
        ),
        (
            tr!("Cuts"),
            vec![
                (tr!("Cut or Put Back"), Box::new(GhostToggle) as Box<dyn Action>),
                (tr!("Cut Without a Trace"), Box::new(StashToOverflow)),
                (tr!("Show Cuts"), Box::new(ToggleOverflow)),
                (tr!("Use the Selected Cut"), Box::new(InsertOverflowNote)),
            ],
        ),
        (
            tr!("Comments"),
            vec![
                (tr!("Add Comment"), Box::new(AddComment) as Box<dyn Action>),
                (tr!("Delete Comment"), Box::new(DeleteComment)),
            ],
        ),
        (
            tr!("Project"),
            vec![
                (tr!("Show the list"), Box::new(ToggleProject) as Box<dyn Action>),
                (tr!("Previous file"), Box::new(PreviousFile)),
                (tr!("Next file"), Box::new(NextFile)),
                (tr!("Find in the project"), Box::new(FindInProject)),
                (tr!("Move to Trash"), Box::new(ListTrash)),
            ],
        ),
        (
            tr!("File"),
            vec![
                (tr!("New"), Box::new(NewDocument) as Box<dyn Action>),
                (tr!("New loose document"), Box::new(NewLooseDocument)),
                (tr!("New project"), Box::new(NewProject)),
                (tr!("Open…"), Box::new(Open)),
                (tr!("Open Most Recent"), Box::new(OpenRecent)),
                (tr!("Save"), Box::new(Save)),
                (tr!("Save As…"), Box::new(SaveAs)),
                (tr!("Copy final text"), Box::new(CopyFinalMarkdown)),
                (tr!("Export"), Box::new(Export)),
                (tr!("Print"), Box::new(Print)),
                (tr!("Markdown source"), Box::new(ViewSource)),
            ],
        ),
        (
            tr!("Editing"),
            vec![
                (tr!("Undo"), Box::new(Undo) as Box<dyn Action>),
                (tr!("Redo"), Box::new(Redo)),
                (tr!("Bold"), Box::new(Bold)),
                (tr!("Italic"), Box::new(Italic)),
                (tr!("Find"), Box::new(Find)),
                (tr!("Find next"), Box::new(FindNext)),
                (tr!("Find previous"), Box::new(FindPrevious)),
                (tr!("Find and replace"), Box::new(FindReplace)),
                (tr!("Match case"), Box::new(ToggleMatchCase)),
                (tr!("Whole words"), Box::new(ToggleWholeWords)),
            ],
        ),
        (
            tr!("Sentences"),
            vec![
                (tr!("Previous sentence"), Box::new(PrevSentence) as Box<dyn Action>),
                (tr!("Next sentence"), Box::new(NextSentence)),
                (tr!("Select to the previous sentence"), Box::new(SelectPrevSentence)),
                (tr!("Select to the next sentence"), Box::new(SelectNextSentence)),
                (tr!("Grow the selection"), Box::new(GrowSelection)),
                (tr!("Shrink the selection"), Box::new(ShrinkSelection)),
                (tr!("Move a sentence or paragraph up"), Box::new(MoveUnitUp)),
                (tr!("Move a sentence or paragraph down"), Box::new(MoveUnitDown)),
            ],
        ),
        (
            tr!("App"),
            vec![
                (tr!("Shortcuts"), Box::new(ShowShortcuts) as Box<dyn Action>),
                (tr!("Preferences"), Box::new(ShowPreferences)),
                (tr!("Full screen"), Box::new(ToggleFullScreen)),
                (tr!("Zen writing"), Box::new(ToggleZenWriting)),
                (tr!("Typewriter scrolling"), Box::new(ToggleTypewriter)),
                (tr!("Private mode"), Box::new(TogglePrivate)),
                (tr!("Dark / light"), Box::new(ToggleAppearance)),
                (tr!("Bigger text"), Box::new(TextBigger)),
                (tr!("Smaller text"), Box::new(TextSmaller)),
                (tr!("Default text size"), Box::new(TextDefaultSize)),
            ],
        ),
    ]
}

/// `project` is what the front window shows: File and View name what's in it. `editing` is the
/// front window's mode, checked in View.
pub fn menus(recent: &[std::path::PathBuf], prefs: &Prefs, project: Option<crate::project::Kind>, editing: bool) -> Vec<Menu> {
    let recent_items: Vec<MenuItem> = if recent.is_empty() {
        vec![MenuItem::action(tr!("No Recent Documents"), NoAction).disabled(true)]
    } else {
        recent
            .iter()
            .map(|p| {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                MenuItem::action(
                    if p.is_dir() { format!("{name}/") } else { name },
                    crate::OpenPath { path: p.to_string_lossy().into_owned() },
                )
            })
            .collect()
    };
    let mut new_items: Vec<MenuItem> = match project {
        Some(crate::project::Kind::Journal) => {
            vec![MenuItem::action(tr!("Today’s Page"), NewDocument), MenuItem::action(tr!("New Document"), NewLooseDocument)]
        }
        Some(kind) => {
            let new = if kind == crate::project::Kind::Book { tr!("New Chapter") } else { tr!("New File") };
            vec![MenuItem::action(new, NewDocument), MenuItem::action(tr!("New Document"), NewLooseDocument)]
        }
        None => vec![MenuItem::action(tr!("New"), NewDocument)],
    };
    new_items.push(MenuItem::action(tr!("New Project…"), NewProject));
    let mut open_items = vec![MenuItem::action(tr!("Open…"), Open)];
    if project.is_none() {
        open_items.push(MenuItem::action(tr!("Open Enclosing Folder"), OpenEnclosingFolder));
    } else {
        open_items.push(MenuItem::action(tr!("Project Settings…"), ProjectSettings));
    }
    let mut view_items: Vec<MenuItem> = Vec::new();
    if let Some(kind) = project {
        use crate::project::Kind;
        let (previous, next) = match kind {
            Kind::Book => (tr!("Previous Chapter"), tr!("Next Chapter")),
            Kind::Journal => (tr!("Previous Day"), tr!("Next Day")),
            Kind::Folder => (tr!("Previous File"), tr!("Next File")),
        };
        view_items.extend([
            MenuItem::action(tr!("Project"), ToggleProject),
            MenuItem::action(previous, PreviousFile),
            MenuItem::action(next, NextFile),
            MenuItem::separator(),
        ]);
    } else {
        view_items.extend([MenuItem::action(tr!("Left Panel"), ToggleProject), MenuItem::separator()]);
    }
    let mut theme_items: Vec<MenuItem> = crate::theme::all()
        .into_iter()
        .map(|(id, theme, _)| MenuItem::action(theme.name, SelectTheme { id: id.clone() }).checked(prefs.theme == id))
        .collect();
    theme_items.extend([MenuItem::separator(), MenuItem::action(tr!("New Theme…"), CreateThemeSheet)]);
    view_items.extend([
        MenuItem::action(tr!("Writing"), WritingMode).checked(!editing),
        MenuItem::action(tr!("Editing"), EditingMode).checked(editing),
        MenuItem::action(tr!("Switch Writing / Editing"), ToggleLayer),
        MenuItem::separator(),
        MenuItem::action(tr!("Zen Writing"), ToggleZenWriting).checked(prefs.zen_writing),
        MenuItem::action(tr!("Typewriter Scrolling"), ToggleTypewriter).checked(prefs.typewriter),
        MenuItem::action(tr!("Private Mode"), TogglePrivate).checked(prefs.private_mode),
        MenuItem::action(tr!("Cuts"), ToggleOverflow),
        MenuItem::action(tr!("Markdown Source"), ViewSource),
        MenuItem::separator(),
        MenuItem::submenu(Menu::new(tr!("Theme")).items(theme_items)),
        MenuItem::submenu(Menu::new(tr!("Appearance")).items([
            MenuItem::action(tr!("Dark"), AppearanceDark).checked(prefs.appearance == Appearance::Dark),
            MenuItem::action(tr!("Light"), AppearanceLight).checked(prefs.appearance == Appearance::Light),
            MenuItem::action(tr!("Match macOS"), AppearanceSystem).checked(prefs.appearance == Appearance::System),
            MenuItem::separator(),
            MenuItem::action(tr!("Toggle Dark / Light"), ToggleAppearance),
        ])),
        MenuItem::submenu(Menu::new(tr!("Text")).items({
            let mut text_items = vec![
                MenuItem::action(tr!("Bigger"), TextBigger).disabled(prefs.font_size >= Prefs::TEXT_SIZES[3]),
                MenuItem::action(tr!("Smaller"), TextSmaller).disabled(prefs.font_size <= Prefs::TEXT_SIZES[0]),
                MenuItem::separator(),
                MenuItem::action(tr!("Default Size"), TextDefaultSize),
                MenuItem::separator(),
            ];
            text_items.extend([
                MenuItem::action(trc!("alignment", "Left"), AlignLeft).checked(prefs.alignment == Alignment::Left),
                MenuItem::action(tr!("Justified"), AlignJustified).checked(prefs.alignment == Alignment::Justified),
                MenuItem::separator(),
                MenuItem::action(tr!("Hyphenate"), ToggleHyphenation).checked(prefs.hyphenation),
                MenuItem::separator(),
                MenuItem::action(tr!("Spaced Paragraphs"), ParagraphsSpaced).checked(prefs.paragraphs == ParagraphLook::Spaced),
                MenuItem::action(tr!("Indented Paragraphs"), ParagraphsIndented).checked(prefs.paragraphs == ParagraphLook::Indented),
            ]);
            text_items
        })),
        MenuItem::separator(),
        MenuItem::action(tr!("Keyboard Shortcuts"), ShowShortcuts),
        MenuItem::separator(),
        MenuItem::action(tr!("Enter Full Screen"), ToggleFullScreen),
    ]);
    let mut file_items = new_items;
    file_items.extend(open_items);
    file_items.extend([
        MenuItem::submenu(Menu::new(tr!("Open Recent")).items(recent_items)),
        MenuItem::action(tr!("Open Most Recent"), OpenRecent),
        MenuItem::separator(),
        MenuItem::action(tr!("Close"), CloseWindow),
        MenuItem::action(tr!("Save"), Save),
        MenuItem::action(tr!("Save As…"), SaveAs),
        MenuItem::action(tr!("Revert to Saved"), RevertToSaved),
        MenuItem::separator(),
        MenuItem::action(tr!("Copy Final Text"), CopyFinalMarkdown),
        MenuItem::action(tr!("Export…"), Export),
        MenuItem::action(tr!("Print…"), Print),
    ]);
    if project.is_some() {
        file_items.extend([MenuItem::separator(), MenuItem::action(tr!("Move to Trash"), ListTrash)]);
    }
    let mut find_items = vec![
        MenuItem::action(tr!("Find…"), Find),
        MenuItem::action(tr!("Find and Replace…"), FindReplace),
        MenuItem::action(tr!("Find Next"), FindNext),
        MenuItem::action(tr!("Find Previous"), FindPrevious),
        MenuItem::action(tr!("Use Selection for Find"), UseSelectionForFind),
        MenuItem::separator(),
        MenuItem::action(tr!("Match Case"), ToggleMatchCase),
        MenuItem::action(tr!("Whole Words"), ToggleWholeWords),
    ];
    if project.is_some() {
        find_items.extend([MenuItem::separator(), MenuItem::action(tr!("Find in Project…"), FindInProject)]);
    }
    // Review needs no model, so it stays when AI is off.
    let ai = prefs.ai.enabled;
    let mut alternatives_items = vec![MenuItem::action(tr!("Alternatives for Selection"), AlternativesForSelection)];
    if ai {
        alternatives_items.push(MenuItem::action(tr!("AI Alternatives for Selection"), AiAlternatives));
    }
    alternatives_items.extend([
        MenuItem::separator(),
        MenuItem::action(tr!("Next Variant"), CycleAtCaretNext),
        MenuItem::action(tr!("Previous Variant"), CycleAtCaretPrev),
        MenuItem::action(tr!("Back to Original"), BackToOriginal),
        MenuItem::action(tr!("Next Group"), NextGroup),
        MenuItem::action(tr!("Previous Group"), PrevGroup),
        MenuItem::separator(),
        MenuItem::action(tr!("Settle on This"), SettleOnThis),
        MenuItem::separator(),
        MenuItem::action(tr!("All Alternatives"), AllAlternatives),
        MenuItem::action(tr!("Previous Group in the Panel"), SidebarPrevGroup),
        MenuItem::action(tr!("Next Group in the Panel"), SidebarNextGroup),
    ]);
    alternatives_items.extend([
        MenuItem::separator(),
        MenuItem::action(tr!("Review…"), ToggleLab),
        MenuItem::action(tr!("Next Mark"), NextMark),
        MenuItem::action(tr!("Previous Mark"), PrevMark),
    ]);
    let mut app_items = vec![MenuItem::action(tr!("About Dostoevsky"), About)];
    #[cfg(feature = "updates")]
    app_items.push(MenuItem::action(tr!("Check for Updates…"), CheckForUpdates));
    app_items.push(MenuItem::separator());
    vec![
        Menu::new("Dostoevsky").items(app_items.into_iter().chain([
            MenuItem::action(tr!("Preferences…"), ShowPreferences),
            MenuItem::action(tr!("Open keymap.json"), OpenKeymapFile),
            MenuItem::separator(),
            MenuItem::os_submenu(tr!("Services"), SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action(tr!("Hide Dostoevsky"), Hide),
            MenuItem::action(tr!("Hide Others"), HideOthers),
            MenuItem::action(tr!("Show All"), ShowAll),
            MenuItem::separator(),
            MenuItem::action(tr!("Quit Dostoevsky"), Quit),
        ])),
        Menu::new(tr!("File")).items(file_items),
        Menu::new(tr!("Edit")).items([
            MenuItem::os_action(tr!("Undo"), Undo, OsAction::Undo),
            MenuItem::os_action(tr!("Redo"), Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action(tr!("Cut"), Cut, OsAction::Cut),
            MenuItem::os_action(tr!("Copy"), Copy, OsAction::Copy),
            MenuItem::os_action(tr!("Paste"), Paste, OsAction::Paste),
            MenuItem::os_action(tr!("Select All"), SelectAll, OsAction::SelectAll),
            MenuItem::separator(),
            MenuItem::action(tr!("Grow Selection"), GrowSelection),
            MenuItem::action(tr!("Shrink Selection"), ShrinkSelection),
            MenuItem::action(tr!("Previous Sentence"), PrevSentence),
            MenuItem::action(tr!("Next Sentence"), NextSentence),
            MenuItem::action(tr!("Move Up"), MoveUnitUp),
            MenuItem::action(tr!("Move Down"), MoveUnitDown),
            MenuItem::separator(),
            MenuItem::action(tr!("Bold"), Bold),
            MenuItem::action(tr!("Italic"), Italic),
            MenuItem::separator(),
            MenuItem::submenu(Menu::new(tr!("Find")).items(find_items)),
            MenuItem::separator(),
            MenuItem::action(tr!("Cut or Put Back"), GhostToggle),
            MenuItem::action(tr!("Delete All Cuts on This Page…"), RemoveAllGhosted),
            MenuItem::action(tr!("Cut Without a Trace"), StashToOverflow),
            MenuItem::action(tr!("Use the Selected Cut"), InsertOverflowNote),
            MenuItem::separator(),
            MenuItem::action(tr!("Add Comment"), AddComment),
            MenuItem::action(tr!("Delete All Comments on This Page…"), DeleteAllComments),
            MenuItem::separator(),
            MenuItem::action(tr!("Emoji & Symbols"), ShowCharacterPalette),
        ]),
        Menu::new(tr!("Alternatives")).items(alternatives_items),
        Menu::new(tr!("View")).items(view_items),
        // The vendored GPUI finds the Window menu by this title.
        Menu::new(tr!("Window")).items([
            MenuItem::action(tr!("Minimize"), Minimize),
            MenuItem::action(tr!("Zoom"), Zoom),
        ]),
        Menu::new(tr!("Help")).items([
            MenuItem::action(tr!("Welcome to Dostoevsky"), ShowWelcome),
            MenuItem::separator(),
            MenuItem::action(tr!("Keyboard Shortcuts"), ShowShortcuts),
        ]),
    ]
}
