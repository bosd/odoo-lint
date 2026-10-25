//! `odl pot --merge`: bring a module's `.po` files in line with its template,
//! as `msgmerge --no-fuzzy-matching` does: the template decides which
//! messages exist and where they come from, the `.po` file keeps its
//! translations, its header and its translators' comments.

use crate::po::{PoEntry, PoFile};
use std::collections::{HashMap, HashSet};

/// What `merge` does with messages the template no longer has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Obsolete {
    /// Keep them as obsolete entries (`#~`), as `msgmerge` does.
    Keep,
    /// Leave them out (`msgattrib --no-obsolete`).
    Drop,
}

type Key<'a> = (Option<&'a str>, &'a str);

fn key(entry: &PoEntry) -> Key<'_> {
    (entry.msgctxt.as_deref(), &entry.msgid)
}

fn is_translated(entry: &PoEntry) -> bool {
    !entry.msgstr.is_empty() || !entry.msgstr_plural.is_empty()
}

/// The entry of `po` for each message: the first live one, else the first
/// obsolete one (as Odoo, which skips obsolete entries, would read it).
fn translations(po: &PoFile) -> HashMap<Key<'_>, &PoEntry> {
    let mut found: HashMap<Key, &PoEntry> = HashMap::new();
    for entry in po.entries.iter().filter(|e| !e.obsolete) {
        found.entry(key(entry)).or_insert(entry);
    }
    for entry in po.entries.iter().filter(|e| e.obsolete) {
        found.entry(key(entry)).or_insert(entry);
    }
    found
}

/// The translation `existing` gives a message of the template. A plural
/// translation of a message that is no longer plural keeps its first form,
/// marked fuzzy, as `msgmerge` does.
fn translate(entry: &mut PoEntry, existing: &PoEntry) {
    entry.tcomment.clone_from(&existing.tcomment);
    for flag in &existing.flags {
        if !entry.flags.contains(flag) {
            entry.flags.push(flag.clone());
        }
    }
    if entry.msgid_plural.is_empty() && !existing.msgstr_plural.is_empty() {
        entry.msgstr = existing.msgstr_plural.values().next().cloned().unwrap_or_default();
        if !entry.flags.iter().any(|f| f == "fuzzy") {
            entry.flags.push("fuzzy".into());
        }
    } else {
        entry.msgstr.clone_from(&existing.msgstr);
        entry.msgstr_plural.clone_from(&existing.msgstr_plural);
    }
}

/// `po` merged with `template`: an entry per message of the template, with
/// the translation, flags and translator comment `po` has for it (none for
/// a new message), and the references and extracted comments of the
/// template. Translated messages only `po` has become obsolete or go, as
/// `obsolete` says.
pub fn merge(po: &PoFile, template: &PoFile, obsolete: Obsolete) -> PoFile {
    let translated = translations(po);
    let mut entries: Vec<PoEntry> = template
        .entries
        .iter()
        .filter(|e| !e.obsolete)
        .map(|message| {
            let mut entry = PoEntry {
                msgid: message.msgid.clone(),
                msgid_plural: message.msgid_plural.clone(),
                msgctxt: message.msgctxt.clone(),
                comment: message.comment.clone(),
                occurrences: message.occurrences.clone(),
                flags: message.flags.clone(),
                ..PoEntry::default()
            };
            if let Some(existing) = translated.get(&key(message)) {
                translate(&mut entry, existing);
            }
            entry
        })
        .collect();
    if obsolete == Obsolete::Keep {
        let mut seen: HashSet<Key> = template.entries.iter().map(key).collect();
        let mut removed: Vec<&PoEntry> = po
            .entries
            .iter()
            .filter_map(|e| translated.get(&key(e)).copied())
            .filter(|e| is_translated(e))
            .collect();
        removed.retain(|e| seen.insert(key(e)));
        entries.extend(removed.into_iter().map(|e| PoEntry {
            obsolete: true,
            occurrences: Vec::new(),
            comment: String::new(),
            previous_msgctxt: None,
            previous_msgid: None,
            previous_msgid_plural: None,
            ..e.clone()
        }));
    }
    PoFile {
        header: po.header.clone(),
        metadata: po.metadata.clone(),
        metadata_is_fuzzy: po.metadata_is_fuzzy,
        entries,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "msgid \"\"\nmsgstr \"\"\n\"Language: nl\\n\"\n\n";

    fn parse(body: &str) -> PoFile {
        PoFile::parse(&format!("{HEADER}{body}")).expect("valid PO")
    }

    fn template() -> PoFile {
        parse(
            "#. module: acme\n#: code:addons/acme/models/a.py:0\nmsgid \"Kept\"\nmsgstr \"\"\n\n\
             #. module: acme\n#: model:ir.ui.menu,name:acme.menu\nmsgid \"New\"\nmsgstr \"\"\n",
        )
    }

    fn translations() -> PoFile {
        parse(
            "# Checked by the customer\n#. module: acme\n#: code:addons/acme/old.py:0\n#, fuzzy\nmsgid \"Kept\"\nmsgstr \"Behouden\"\n\n\
             #. module: acme\nmsgid \"Gone\"\nmsgstr \"Weg\"\n\n\
             #. module: acme\nmsgid \"Never translated\"\nmsgstr \"\"\n",
        )
    }

    fn entry<'a>(po: &'a PoFile, msgid: &str) -> &'a PoEntry {
        po.entries.iter().find(|e| e.msgid == msgid).expect("entry")
    }

    #[test]
    fn keeps_the_translation_flags_and_translator_comment() {
        let merged = merge(&translations(), &template(), Obsolete::Keep);
        let kept = entry(&merged, "Kept");
        assert_eq!(
            (kept.msgstr.as_str(), kept.flags.as_slice(), kept.tcomment.as_str()),
            ("Behouden", ["fuzzy".to_string()].as_slice(), "Checked by the customer")
        );
    }

    #[test]
    fn takes_the_references_of_the_template() {
        let merged = merge(&translations(), &template(), Obsolete::Keep);
        assert_eq!(
            entry(&merged, "Kept").occurrences,
            [("code:addons/acme/models/a.py".to_string(), "0".to_string())]
        );
    }

    #[test]
    fn adds_new_messages_untranslated() {
        let merged = merge(&translations(), &template(), Obsolete::Keep);
        assert_eq!(entry(&merged, "New").msgstr, "");
    }

    #[test]
    fn keeps_translations_of_removed_messages_as_obsolete() {
        let merged = merge(&translations(), &template(), Obsolete::Keep);
        assert!(entry(&merged, "Gone").obsolete);
    }

    #[test]
    fn forgets_removed_messages_without_a_translation() {
        let merged = merge(&translations(), &template(), Obsolete::Keep);
        assert!(!merged.entries.iter().any(|e| e.msgid == "Never translated"));
    }

    #[test]
    fn drops_removed_messages_when_asked() {
        let merged = merge(&translations(), &template(), Obsolete::Drop);
        assert!(!merged.entries.iter().any(|e| e.msgid == "Gone"));
    }

    #[test]
    fn prefers_the_live_translation_to_an_obsolete_one() {
        let po = parse("msgid \"Kept\"\nmsgstr \"Live\"\n\n#~ msgid \"Kept\"\n#~ msgstr \"Old\"\n");
        assert_eq!(entry(&merge(&po, &template(), Obsolete::Keep), "Kept").msgstr, "Live");
    }

    #[test]
    fn prefers_the_first_of_duplicate_translations() {
        let po = parse("msgid \"Kept\"\nmsgstr \"First\"\n\nmsgid \"Kept\"\nmsgstr \"Second\"\n");
        assert_eq!(entry(&merge(&po, &template(), Obsolete::Keep), "Kept").msgstr, "First");
    }

    #[test]
    fn keeps_one_obsolete_entry_for_a_removed_duplicate() {
        let po = parse("msgid \"Gone\"\nmsgstr \"Weg\"\n\n#~ msgid \"Gone\"\n#~ msgstr \"Oud\"\n");
        let merged = merge(&po, &template(), Obsolete::Keep);
        assert_eq!(merged.entries.iter().filter(|e| e.msgid == "Gone").count(), 1);
    }

    #[test]
    fn revives_an_obsolete_translation_the_template_has_again() {
        let po = parse("#~ msgid \"New\"\n#~ msgstr \"Nieuw\"\n");
        let merged = merge(&po, &template(), Obsolete::Keep);
        assert_eq!(
            (entry(&merged, "New").msgstr.as_str(), entry(&merged, "New").obsolete),
            ("Nieuw", false)
        );
    }

    #[test]
    fn turns_a_plural_translation_of_a_singular_message_into_a_fuzzy_one() {
        let po = parse("msgid \"Kept\"\nmsgid_plural \"Kepts\"\nmsgstr[0] \"Een\"\nmsgstr[1] \"Meer\"\n");
        let merged = merge(&po, &template(), Obsolete::Keep);
        let kept = entry(&merged, "Kept");
        assert_eq!(
            (
                kept.msgstr.as_str(),
                kept.msgstr_plural.is_empty(),
                kept.flags.as_slice()
            ),
            ("Een", true, ["fuzzy".to_string()].as_slice())
        );
    }

    #[test]
    fn keeps_the_header_of_the_translations() {
        let merged = merge(&translations(), &template(), Obsolete::Keep);
        assert_eq!(merged.metadata, translations().metadata);
    }
}
