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
| [C8111](C8111.md) | `development-status-allowed` | The manifest `development_status` is not an allowed value. |
| [C8112](C8112.md) | `missing-readme` | The module has no README. |
| [C8114](C8114.md) | `category-allowed` | The manifest category is not in the allowed list. |
| [C8115](C8115.md) | `missing-odoo-file` | A file required for paid apps is missing (configurable list). |
| [C8116](C8116.md) | `manifest-superfluous-key` | A manifest key is set to its default value. |
| [C8117](C8117.md) | `category-allowed-app` | The category of a paid app is not an Odoo Apps store category. |
| [C8118](C8118.md) | `missing-odoo-file-app` | A paid app has no `static/description/index.html`. |
| [C8119](C8119.md) | `manifest-required-key-app` | A key required for paid apps is missing from the manifest. |
| [C8120](C8120.md) | `manifest-summary-multiline` | The manifest summary spans several lines. |
| [E0001](E0001.md) | `syntax-error` | A Python file cannot be parsed. |
| [E8101](E8101.md) | `manifest-author-string` | The manifest `author` is not a string. |
| [E8104](E8104.md) | `manifest-maintainers-list` | The manifest `maintainers` is not a list of strings. |
| [E8145](E8145.md) | `manifest-behind-migrations` | The manifest version is lower than a migration folder. |
| [F8101](F8101.md) | `resource-not-exist` | A data file listed in the manifest does not exist. |
| [ODOO001](ODOO001.md) | `missing-depends` | Compute method referenced by `compute=` lacks `@api.depends`. |
| [R8181](R8181.md) | `invalid-email` | The manifest `support` is not a valid email address. |
| [W8114](W8114.md) | `website-manifest-key-not-valid-uri` | The manifest `website` is not a valid http(s) URL. |
| [W8125](W8125.md) | `manifest-data-duplicated` | A data file is listed more than once in the manifest. |
| [W8162](W8162.md) | `manifest-external-assets` | An asset in the manifest is loaded from an external URL. |

```{toctree}
:hidden:

C8101
C8102
C8103
C8105
C8106
C8111
C8112
C8114
C8115
C8116
C8117
C8118
C8119
C8120
E0001
E8101
E8104
E8145
F8101
ODOO001
R8181
W8114
W8125
W8162
```
