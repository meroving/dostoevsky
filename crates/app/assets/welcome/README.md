# The welcome page (dsky-l4t.3)

One file per UI language (`en.md`, `ru.md`, `de.md`, `fr.md`, `es.md`, `it.md`, `pt-BR.md`),
built into the app and turned into a document by `crates/app/src/welcome.rs` each time it opens
(first launch, Help → Welcome to Dostoevsky). Plain Markdown plus a little markup for what the
page carries:

| Markup | Becomes |
|---|---|
| `{{garret|closet|cubbyhole}}` | an alternative group, the first version showing (a word group when it has no spaces, else a sentence group) |
| `[[words :: comment]]` | a comment on *words* (which may hold a group) |
| `~~text~~` | a cut shown faded in its place |
| `<<text>>` | a cut with no trace, waiting in the Cuts panel |
| `[key:ActionName]` | the action's keys from the live keymap (`welcome::action_named`) |
| `‸` | where the caret starts |

Write each language in its own words, not as a translation of the English, and name the UI's
labels exactly as that language's catalog has them. Check a new or changed text with LanguageTool
and a native reader (`welcome`, “Writing it in seven languages”). The word group's
versions share one gender and case, so the words around it still agree. `welcome::tests` checks
every file: the parts are all there, every key resolves, the long sentence is over
`ai::lab::LONG_SENTENCE` and the room word echoes.
