"""Sphinx configuration for the odoo-lint documentation."""

project = "odoo-lint"
author = "bosd"
copyright = f"2026, {author}"

extensions = [
    "myst_parser",
    "sphinx_copybutton",
]

html_theme = "shibuya"
html_title = "odoo-lint"
html_static_path = ["_static"]
html_logo = "_static/logo.svg"
html_favicon = "_static/favicon.png"

html_theme_options = {
    "accent_color": "orange",
    "github_url": "https://github.com/bosd/odoo-lint",
}

myst_enable_extensions = [
    "colon_fence",
    "deflist",
]
# Generate anchors for headings so cross-page links like rules/ODOO001.md#example work.
myst_heading_anchors = 3
