<!-- Generated from the rule sources; run `UPDATE_DOCS=1 cargo test --test generated_docs` -->

# Rules

Run `odl rule <CODE>` to show the same documentation in the terminal.

| Code | Name | Summary |
| ---- | ---- | ------- |
| [C8101](C8101.md) | `manifest-required-author` | None of the required authors is in the manifest `author`. |
| [C8102](C8102.md) | `manifest-required-key` | A required key is missing from the manifest. |
| [C8103](C8103.md) | `manifest-deprecated-key` | The manifest uses a deprecated key. |
| [C8105](C8105.md) | `license-allowed` | The manifest license is not in the allowed list. |
| [C8106](C8106.md) | `manifest-version-format` | The manifest version does not follow `<odoo series>.x.y.z`. |
| [C8107](C8107.md) | `translation-required` | A user-facing string is not translated. |
| [C8108](C8108.md) | `method-compute` | A field's `compute` method is not named `_compute_...`. |
| [C8109](C8109.md) | `method-search` | A field's `search` method is not named `_search_...`. |
| [C8110](C8110.md) | `method-inverse` | A field's `inverse` method is not named `_inverse_...`. |
| [C8111](C8111.md) | `development-status-allowed` | The manifest `development_status` is not an allowed value. |
| [C8112](C8112.md) | `missing-readme` | The module has no README. |
| [C8113](C8113.md) | `no-wizard-in-models` | A wizard (`TransientModel`) is defined in the `models` folder. |
| [C8114](C8114.md) | `category-allowed` | The manifest category is not in the allowed list. |
| [C8115](C8115.md) | `missing-odoo-file` | A file required for paid apps is missing (configurable list). |
| [C8116](C8116.md) | `manifest-superfluous-key` | A manifest key is set to its default value. |
| [C8117](C8117.md) | `category-allowed-app` | The category of a paid app is not an Odoo Apps store category. |
| [C8118](C8118.md) | `missing-odoo-file-app` | A paid app has no `static/description/index.html`. |
| [C8119](C8119.md) | `manifest-required-key-app` | A key required for paid apps is missing from the manifest. |
| [C8120](C8120.md) | `manifest-summary-multiline` | The manifest summary spans several lines. |
| [E0001](E0001.md) | `syntax-error` | A Python file cannot be parsed. |
| [E8101](E8101.md) | `manifest-author-string` | The manifest `author` is not a string. |
| [E8102](E8102.md) | `invalid-commit` | The database transaction is committed by hand. |
| [E8103](E8103.md) | `sql-injection` | SQL passed to `execute()` is built with string formatting. |
| [E8104](E8104.md) | `manifest-maintainers-list` | The manifest `maintainers` is not a list of strings. |
| [E8106](E8106.md) | `external-request-timeout` | An external request has no `timeout`. |
| [E8130](E8130.md) | `test-folder-imported` | `tests` is imported from a package `__init__.py`. |
| [E8135](E8135.md) | `no-write-in-compute` | A compute method calls `write()`. |
| [E8140](E8140.md) | `no-raise-unlink` | `unlink()` raises an exception. |
| [E8145](E8145.md) | `manifest-behind-migrations` | The manifest version is lower than a migration folder. |
| [E8146](E8146.md) | `deprecated-name-get` | A model defines `name_get`, replaced by `_compute_display_name`. |
| [E8147](E8147.md) | `inheritable-method-string` | A field passes a method object instead of its name. |
| [E8148](E8148.md) | `inheritable-method-lambda` | A field passes a method object as `default` or `domain`. |
| [E8149](E8149.md) | `deprecated-inselect-operator` | A domain uses the deprecated `inselect` operator. |
| [E8151](E8151.md) | `translation-injection` | `.format()` is called on a translated string. |
| [E8300](E8300.md) | `translation-unsupported-format` | A translated string has an invalid `%` conversion character. |
| [E8301](E8301.md) | `translation-format-truncated` | A translated format string ends in the middle of a `%` conversion. |
| [E8305](E8305.md) | `translation-too-many-args` | `_()` gets more arguments than its format string uses. |
| [E8306](E8306.md) | `translation-too-few-args` | `_()` gets fewer arguments than its format string needs. |
| [F8101](F8101.md) | `resource-not-exist` | A data file listed in the manifest does not exist. |
| [ODOO001](ODOO001.md) | `missing-depends` | Compute method referenced by `compute=` lacks `@api.depends`. |
| [PO001](PO001.md) | `po-syntax-error` | A PO file cannot be parsed. |
| [PO002](PO002.md) | `po-requires-module` | A translation entry lacks its `#. module:` comment. |
| [PO003](PO003.md) | `po-python-parse-printf` | A translation does not match the `%` placeholders of its source. |
| [PO004](PO004.md) | `po-python-parse-format` | A translation does not match the `{}` placeholders of its source. |
| [PO005](PO005.md) | `po-duplicate-message-definition` | The same `msgid` is translated more than once. |
| [PO006](PO006.md) | `po-duplicate-model-definition` | The same `model:` reference is translated more than once. |
| [PO007](PO007.md) | `po-pretty-format` | A PO file is not formatted the way Odoo exports it. |
| [PO101](PO101.md) | `po-not-in-pot` | A translation is ignored because its `msgid` is missing from the module's `.pot`. |
| [PO102](PO102.md) | `po-unknown-occurrence` | Odoo cannot read a `#:` reference of a translation. |
| [PO103](PO103.md) | `po-file-name` | Odoo never loads a `.po` file with this name. |
| [PO104](PO104.md) | `po-fuzzy` | A translation is marked `fuzzy`, but Odoo loads it anyway. |
| [R8101](R8101.md) | `odoo-exception-warning` | `odoo.exceptions.Warning` is imported. |
| [R8180](R8180.md) | `consider-merging-classes-inherited` | Several classes in one module extend the same model. |
| [R8181](R8181.md) | `invalid-email` | The manifest `support` is not a valid email address. |
| [U1801](U1801.md) | `upgrade-tree-view` | A view uses `<tree>`, renamed `<list>` in Odoo 18.0. |
| [U1802](U1802.md) | `upgrade-view-mode-tree` | An action's `view_mode` says `tree`, renamed `list` in Odoo 18.0. |
| [U1803](U1803.md) | `upgrade-tree-reference` | An xpath, `mode` or `tree_view_ref` refers to `tree`, renamed `list` in Odoo 18.0. |
| [U1804](U1804.md) | `upgrade-cron-numbercall` | A scheduled action sets `numbercall` or `doall`, removed in Odoo 18.0. |
| [U1805](U1805.md) | `upgrade-default-period` | A date filter's `default_period` uses a name Odoo 18.0 replaced. |
| [U1806](U1806.md) | `upgrade-kanban-box` | A kanban view uses the `kanban-box` template, replaced by `card` in Odoo 18.0. |
| [U1807](U1807.md) | `upgrade-group-operator` | A field uses `group_operator=`, renamed `aggregator=` in Odoo 18.0. |
| [U1808](U1808.md) | `upgrade-user-has-groups` | `user_has_groups()`, removed in Odoo 18.0, is called. |
| [U1809](U1809.md) | `upgrade-python-tree-view` | Python code refers to the `tree` view type, renamed `list` in Odoo 18.0. |
| [U1810](U1810.md) | `upgrade-name-search-override` | A model overrides `_name_search`, which Odoo 18.0 no longer calls. |
| [W8103](W8103.md) | `translation-field` | A field label is wrapped in `_()`. |
| [W8105](W8105.md) | `attribute-deprecated` | A model uses a deprecated class attribute. |
| [W8106](W8106.md) | `method-required-super` | An override of a core method does not call `super()`. |
| [W8107](W8107.md) | `prohibited-method-override` | A method that must not be overridden is overridden. |
| [W8110](W8110.md) | `missing-return` | A method calls `super()` but does not return anything. |
| [W8111](W8111.md) | `renamed-field-parameter` | A field uses a parameter that was renamed. |
| [W8113](W8113.md) | `attribute-string-redundant` | A field label repeats what Odoo derives from the field name. |
| [W8114](W8114.md) | `website-manifest-key-not-valid-uri` | The manifest `website` is not a valid http(s) URL. |
| [W8115](W8115.md) | `translation-contains-variable` | A translated string is formatted inside `_()` (Odoo 13.0 and earlier). |
| [W8116](W8116.md) | `print-used` | `print()` is used instead of a logger. |
| [W8120](W8120.md) | `translation-positional-used` | A translated string has several positional placeholders. |
| [W8121](W8121.md) | `context-overridden` | `with_context()` replaces the whole context. |
| [W8125](W8125.md) | `manifest-data-duplicated` | A data file is listed more than once in the manifest. |
| [W8138](W8138.md) | `except-pass` | An `except` block only contains `pass`. |
| [W8150](W8150.md) | `odoo-addons-relative-import` | A module imports itself through `odoo.addons.<module>`. |
| [W8155](W8155.md) | `bad-builtin-groupby` | `itertools.groupby` is used instead of `odoo.tools.groupby`. |
| [W8160](W8160.md) | `deprecated-odoo-model-method` | A model overrides a method Odoo deprecated. |
| [W8161](W8161.md) | `prefer-env-translation` | `_()` is used instead of `self.env._()`. |
| [W8162](W8162.md) | `manifest-external-assets` | An asset in the manifest is loaded from an external URL. |
| [W8163](W8163.md) | `no-search-all` | `search([])` without a `limit` loads every record. |
| [W8164](W8164.md) | `super-method-mismatch` | A method calls `super()` on a different method. |
| [W8165](W8165.md) | `deprecated-self-cr` | `self._cr` is used instead of `self.env.cr`. |
| [W8202](W8202.md) | `use-vim-comment` | A file contains a vim modeline comment. |
| [W8301](W8301.md) | `translation-not-lazy` | A string is formatted with `%` or `+` before it is translated. |
| [W8302](W8302.md) | `translation-format-interpolation` | A string is formatted with `.format()` before it is translated. |
| [W8303](W8303.md) | `translation-fstring-interpolation` | An f-string is translated. |
| [XML001](XML001.md) | `xml-syntax-error` | An XML file the manifest loads cannot be read or parsed. |
| [XML002](XML002.md) | `xml-header-missing` | An XML file has no `<?xml ... ?>` declaration. |
| [XML003](XML003.md) | `xml-header-wrong` | The XML declaration is not `<?xml version="1.0" encoding="UTF-8" ?>`. |
| [XML004](XML004.md) | `xml-record-missing-id` | A `<record>` or `<menuitem>` has no `id`. |
| [XML005](XML005.md) | `xml-duplicate-record-id` | Two records of a module have the same XML id. |
| [XML006](XML006.md) | `xml-duplicate-fields` | A record sets the same field twice. |
| [XML007](XML007.md) | `xml-duplicate-template-id` | Two templates of a module have the same id. |
| [XML008](XML008.md) | `xml-redundant-module-name` | A record id repeats the name of its own module. |
| [XML009](XML009.md) | `xml-tag-position` | `t-if`, `id` or `class` attributes are not first in a tag. |
| [XML010](XML010.md) | `xml-deprecated-data-node` | A `<data>` element is the only child of `<odoo>`. |
| [XML011](XML011.md) | `xml-deprecated-openerp-node` | The root element is `<openerp>`. |
| [XML012](XML012.md) | `xml-deprecated-qweb-directive` | A template uses `t-esc-options`, `t-field-options` or `t-raw-options`. |
| [XML013](XML013.md) | `xml-deprecated-qweb-directive-15` | A template uses `t-esc` or `t-raw`, deprecated in Odoo 15.0. |
| [XML014](XML014.md) | `xml-deprecated-tree-attribute` | A `<tree>` view uses `string`, `colors` or `fonts`. |
| [XML015](XML015.md) | `xml-deprecated-oe-chatter` | A form uses `<div class="oe_chatter">` instead of `<chatter/>`. |
| [XML016](XML016.md) | `xml-deprecated-res-groups-category-id` | A `res.groups` record sets `category_id`, removed in Odoo 19.0. |
| [XML017](XML017.md) | `xml-view-dangerous-replace-low-priority` | A view replaces part of another with a priority below 99. |
| [XML018](XML018.md) | `xml-dangerous-qweb-replace-low-priority` | A template replaces part of another with a priority below 99. |
| [XML019](XML019.md) | `xml-create-user-wo-reset-password` | A `res.users` record is created without `no_reset_password`. |
| [XML020](XML020.md) | `xml-not-valid-char-link` | A local `href`/`src` has no plain file extension. |
| [XML021](XML021.md) | `xml-xpath-translatable-item` | An `<xpath>` selects an element by its translatable text. |
| [XML022](XML022.md) | `xml-oe-structure-missing-id` | An `oe_structure` element has no id containing `oe_structure`. |
| [XML023](XML023.md) | `xml-field-bool-without-eval` | A boolean field is set as text instead of with `eval`. |
| [XML024](XML024.md) | `xml-field-numeric-without-eval` | A numeric field is set as text instead of with `eval`. |
| [XML101](XML101.md) | `xml-bootstrap4-class` | A Bootstrap 4 class that Bootstrap 5 renamed, in Odoo 15.0 or later. |
| [XML102](XML102.md) | `xml-bootstrap4-removed-class` | A Bootstrap 4 class that Bootstrap 5 removed, in Odoo 15.0 or later. |

```{toctree}
:hidden:

C8101
C8102
C8103
C8105
C8106
C8107
C8108
C8109
C8110
C8111
C8112
C8113
C8114
C8115
C8116
C8117
C8118
C8119
C8120
E0001
E8101
E8102
E8103
E8104
E8106
E8130
E8135
E8140
E8145
E8146
E8147
E8148
E8149
E8151
E8300
E8301
E8305
E8306
F8101
ODOO001
PO001
PO002
PO003
PO004
PO005
PO006
PO007
PO101
PO102
PO103
PO104
R8101
R8180
R8181
U1801
U1802
U1803
U1804
U1805
U1806
U1807
U1808
U1809
U1810
W8103
W8105
W8106
W8107
W8110
W8111
W8113
W8114
W8115
W8116
W8120
W8121
W8125
W8138
W8150
W8155
W8160
W8161
W8162
W8163
W8164
W8165
W8202
W8301
W8302
W8303
XML001
XML002
XML003
XML004
XML005
XML006
XML007
XML008
XML009
XML010
XML011
XML012
XML013
XML014
XML015
XML016
XML017
XML018
XML019
XML020
XML021
XML022
XML023
XML024
XML101
XML102
```
