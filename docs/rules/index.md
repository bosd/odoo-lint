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
| [U1601](U1601.md) | `upgrade-extension-view-groups` | An extension view sets `groups_id`, rejected since Odoo 16.0. |
| [U1602](U1602.md) | `upgrade-html-field-type` | `body_html` of a mail template loaded with `type="xml"`, deprecated in Odoo 16.0. |
| [U1603](U1603.md) | `upgrade-assets-qweb` | The manifest adds templates to `web.assets_qweb`, removed in Odoo 16.0. |
| [U1604](U1604.md) | `upgrade-removed-asset-bundles-16` | The manifest uses an asset bundle removed in Odoo 16.0. |
| [U1605](U1605.md) | `upgrade-manifest-qweb` | The manifest lists templates under `qweb`, which Odoo ignores. |
| [U1606](U1606.md) | `upgrade-ir-translation` | Python code uses `ir.translation`, removed in Odoo 16.0. |
| [U1607](U1607.md) | `upgrade-request-api` | `request.jsonrequest`, or an assignment to `request.context`/`request.uid`, removed in Odoo 16.0. |
| [U1608](U1608.md) | `upgrade-binary-content` | `ir.http.binary_content()`, removed in Odoo 16.0. |
| [U1609](U1609.md) | `upgrade-search-args` | `search(args=...)`, renamed `domain=` in Odoo 16.0. |
| [U1610](U1610.md) | `upgrade-osv-query` | `odoo.osv.query`, moved to `odoo.tools.query` in Odoo 16.0. |
| [U1611](U1611.md) | `upgrade-fields-view-get` | A `fields_view_get` override, no longer called by the web client since Odoo 16.0. |
| [U1701](U1701.md) | `upgrade-attrs-states` | A view uses `attrs` or `states`, rejected since Odoo 17.0. |
| [U1702](U1702.md) | `upgrade-list-column-invisible` | A list column hidden with `invisible`, which only hides the cells since Odoo 17.0. |
| [U1703](U1703.md) | `upgrade-report-act-window-tags` | `<report>` or `<act_window>`, removed in Odoo 17.0. |
| [U1704](U1704.md) | `upgrade-calendar-quick-add` | A calendar view uses `quick_add`, renamed `quick_create` in Odoo 17.0. |
| [U1705](U1705.md) | `upgrade-view-active-id` | A view expression uses `active_id` directly, deprecated in 17.0 and rejected in 18.0. |
| [U1706](U1706.md) | `upgrade-server-action-lines` | A server action uses `fields_lines`/`ir.server.object.lines`, removed in Odoo 17.0. |
| [U1707](U1707.md) | `upgrade-view-field-parent` | A view record sets `field_parent`, removed in Odoo 17.0. |
| [U1708](U1708.md) | `upgrade-view-qweb-directives` | A view uses a QWeb directive Odoo 17.0 forbids in views. |
| [U1709](U1709.md) | `upgrade-name-get` | `name_get`, no longer used for display names since Odoo 17.0. |
| [U1710](U1710.md) | `upgrade-name-search-signature` | A `_name_search` override with the 16.0 signature (`args`, `name_get_uid`). |
| [U1711](U1711.md) | `upgrade-search-count-true` | `search(..., count=True)`, removed in Odoo 17.0. |
| [U1712](U1712.md) | `upgrade-removed-recordset-methods` | A recordset method deprecated in 16.0 and removed in 17.0. |
| [U1713](U1713.md) | `upgrade-field-states` | A field uses `states=`, ignored since Odoo 17.0. |
| [U1714](U1714.md) | `upgrade-savepoint-case` | `SavepointCase`/`HttpSavepointCase`, removed in Odoo 17.0. |
| [U1715](U1715.md) | `upgrade-old-exceptions` | `odoo.exceptions.Warning` or `except_orm`, removed in Odoo 17.0. |
| [U1716](U1716.md) | `upgrade-onchange-domain` | An onchange returns a `domain`, ignored since Odoo 17.0. |
| [U1717](U1717.md) | `upgrade-norecompute` | `env.norecompute()`, a no-op since Odoo 17.0. |
| [U1718](U1718.md) | `upgrade-ir-default-get` | `ir.default.get()`, renamed `_get()` in Odoo 17.0. |
| [U1719](U1719.md) | `upgrade-private-read-group` | `_read_group` called with the 16.0 signature (`lazy`, `orderby`, fields). |
| [U1720](U1720.md) | `upgrade-openerp-manifest` | The manifest is `__openerp__.py`, deprecated in 17.0 and not found by 19.0. |
| [U1721](U1721.md) | `upgrade-removed-asset-bundles` | The manifest adds files to an asset bundle removed in Odoo 17.0. |
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
| [U1901](U1901.md) | `upgrade-env-shortcuts` | `._uid`, `._context` or `request.cr/uid/context`, deprecated in Odoo 19.0. |
| [U1902](U1902.md) | `upgrade-sql-constraints` | `_sql_constraints`, silently ignored since Odoo 19.0. |
| [U1903](U1903.md) | `upgrade-api-model-create` | `create` is decorated with `@api.model`, which makes it a batch create in Odoo 19.0. |
| [U1904](U1904.md) | `upgrade-api-returns` | `@api.returns`, removed in Odoo 19.0. |
| [U1905](U1905.md) | `upgrade-read-group-override` | A model overrides `read_group`, which the web client no longer calls in Odoo 19.0. |
| [U1906](U1906.md) | `upgrade-route-json` | A route uses `type='json'`, renamed `'jsonrpc'` in Odoo 19.0. |
| [U1907](U1907.md) | `upgrade-python-groups-id` | Python code uses `groups_id`, renamed `group_ids` in Odoo 19.0. |
| [U1908](U1908.md) | `upgrade-osv-expression` | `odoo.osv.expression` is imported, deprecated in Odoo 19.0 and removed in 20.0. |
| [U1909](U1909.md) | `upgrade-auto-join` | A field uses `auto_join=`, renamed `bypass_search_access=` in Odoo 19.0. |
| [U1910](U1910.md) | `upgrade-name-search-args` | `name_search(args=...)`, renamed `domain=` in Odoo 19.0. |
| [U1911](U1911.md) | `upgrade-clear-caches` | `clear_caches()`, removed in Odoo 19.0. |
| [U1912](U1912.md) | `upgrade-removed-helpers` | `get_module_resource`, `get_resource_path` or `odoo.registry()`, removed in Odoo 19.0. |
| [U1913](U1913.md) | `upgrade-sequence-get` | `ir.sequence` `get()`/`get_id()`, removed in Odoo 19.0. |
| [U1914](U1914.md) | `upgrade-domain-operators` | A domain uses `<>`, `==` or an upper-case operator, deprecated in Odoo 19.0. |
| [U1915](U1915.md) | `upgrade-models-newid` | `from odoo.models import NewId`, which fails in Odoo 19.0. |
| [U1916](U1916.md) | `upgrade-groups-id` | A record or view uses `groups_id`, renamed `group_ids` in Odoo 19.0. |
| [U1917](U1917.md) | `upgrade-search-group-attributes` | A search view's `<group>` has `expand` or `string`, rejected in Odoo 19.0. |
| [U1918](U1918.md) | `upgrade-groups-users` | A `res.groups` record sets `users`, renamed `user_ids` in Odoo 19.0. |
| [U1919](U1919.md) | `upgrade-manifest-old-data-keys` | The manifest lists files under `update_xml` or `demo_xml`, ignored since Odoo 19.0. |
| [U1920](U1920.md) | `upgrade-t-call-element` | `t-call` on an element other than `<t>`, rejected in Odoo 19.0. |
| [U1921](U1921.md) | `upgrade-partner-mobile-title` | A partner or company view uses `mobile` or `title`, removed in Odoo 19.0. |
| [U2001](U2001.md) | `upgrade-ir-model-access-csv` | The manifest loads `ir.model.access.csv`, a model replaced by `ir.access` in Odoo 20.0. |
| [U2002](U2002.md) | `upgrade-ir-access-records` | `ir.rule` or `ir.model.access` records, models replaced by `ir.access` in Odoo 20.0. |
| [U2003](U2003.md) | `upgrade-t-esc-t-raw` | `t-esc` or `t-raw`, removed from QWeb in Odoo 20.0. |
| [U2004](U2004.md) | `upgrade-t-call-body` | `t-set` inside a `t-call`, no longer passed to the template in Odoo 20.0. |
| [U2005](U2005.md) | `upgrade-t-call-options` | `t-call-options`, removed in Odoo 20.0. |
| [U2006](U2006.md) | `upgrade-base64-file-field` | `<field type="base64" file=...>`, deprecated for `type="bytes"` in Odoo 20.0. |
| [U2007](U2007.md) | `upgrade-attachment-datas-xml` | An `ir.attachment` record sets `datas`, removed in Odoo 20.0. |
| [U2008](U2008.md) | `upgrade-report-file` | A report sets `report_file`, removed in Odoo 20.0. |
| [U2009](U2009.md) | `upgrade-font-awesome` | A Font Awesome icon, which Odoo 20.0 no longer loads. |
| [U2010](U2010.md) | `upgrade-filter-date-range` | A search filter uses `start_month`/`end_month`/`start_year`/`end_year`, removed in Odoo 20.0. |
| [U2011](U2011.md) | `upgrade-calendar-date-delay` | A calendar view uses `date_delay`, removed in Odoo 20.0. |
| [U2012](U2012.md) | `upgrade-bank-account-fields` | A `res.partner.bank` view or record uses a field renamed in Odoo 20.0. |
| [U2013](U2013.md) | `upgrade-partner-company-fields` | A partner or company view uses `company_type`, `company_name` or `company_registry`, removed in Odoo 20.0. |
| [U2014](U2014.md) | `upgrade-widget-renames` | `widget="remaining_days"`, renamed `relative_date` in Odoo 20.0. |
| [U2015](U2015.md) | `upgrade-jquery-bundle` | The manifest includes `web._assets_jquery`, removed in Odoo 20.0. |
| [U2016](U2016.md) | `upgrade-manifest-init-xml` | The manifest lists files under `init_xml`, ignored since Odoo 20.0. |
| [U2017](U2017.md) | `upgrade-config-parameter` | `ir.config_parameter` `get_param`/`set_param`, removed in Odoo 20.0. |
| [U2018](U2018.md) | `upgrade-attachment-datas` | `ir.attachment` `datas`, removed in Odoo 20.0. |
| [U2019](U2019.md) | `upgrade-ir-access-models` | Python code uses `ir.model.access` or `ir.rule`, replaced by `ir.access` in Odoo 20.0. |
| [U2020](U2020.md) | `upgrade-removed-access-methods` | An access or recursion method removed in Odoo 20.0. |
| [U2021](U2021.md) | `upgrade-read-group-signature` | `read_group` called with the 19.0 signature (`lazy`, `orderby`, `fields`). |
| [U2022](U2022.md) | `upgrade-registry-clear-cache` | `registry.clear_cache()`, removed in Odoo 20.0. |
| [U2023](U2023.md) | `upgrade-ormcache-import` | `ormcache` imported from `odoo.tools`, deprecated in Odoo 20.0. |
| [U2024](U2024.md) | `upgrade-http-imports` | A name imported from `odoo.http` that moved to a submodule in Odoo 20.0. |
| [U2025](U2025.md) | `upgrade-tools-imports` | A name imported from `odoo.tools` that moved or was removed in Odoo 20.0. |
| [U2026](U2026.md) | `upgrade-bank-account-fields-python` | Python code uses a `res.partner.bank` field renamed in Odoo 20.0. |
| [U2027](U2027.md) | `upgrade-partner-company-fields-python` | Python code uses `company_type`, `company_registry` or `create_company`, removed in Odoo 20.0. |
| [U2028](U2028.md) | `upgrade-self-writeable-fields` | `SELF_READABLE_FIELDS`/`SELF_WRITEABLE_FIELDS`, replaced in Odoo 20.0. |
| [U2029](U2029.md) | `upgrade-inherit-read` | Code reads `._inherit` at runtime, which raises in Odoo 20.0. |
| [U2030](U2030.md) | `upgrade-test-classes` | `SingleTransactionCase` or `odoo.tests.common.Form`, removed in Odoo 20.0. |
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
U1601
U1602
U1603
U1604
U1605
U1606
U1607
U1608
U1609
U1610
U1611
U1701
U1702
U1703
U1704
U1705
U1706
U1707
U1708
U1709
U1710
U1711
U1712
U1713
U1714
U1715
U1716
U1717
U1718
U1719
U1720
U1721
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
U1901
U1902
U1903
U1904
U1905
U1906
U1907
U1908
U1909
U1910
U1911
U1912
U1913
U1914
U1915
U1916
U1917
U1918
U1919
U1920
U1921
U2001
U2002
U2003
U2004
U2005
U2006
U2007
U2008
U2009
U2010
U2011
U2012
U2013
U2014
U2015
U2016
U2017
U2018
U2019
U2020
U2021
U2022
U2023
U2024
U2025
U2026
U2027
U2028
U2029
U2030
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
