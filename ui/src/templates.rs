// Node-type Markdown templates.
//
// Applied once when a new node is created; the user owns the body afterwards.
// Structure only — headings and empty bullets, never instructional prose:
// prose placeholders get saved as real body text and later read (and export)
// as content nobody wrote (#98 item 5).
// Templates omit a To-Do section — structured tasks are managed in the task
// panel on the node view, not in free-form Markdown.

pub fn template_for_type(node_type: &str) -> &'static str {
    match node_type {
        "project" => PROJECT,
        "area" => AREA,
        "resource" => RESOURCE,
        "reference" => REFERENCE,
        _ => "",
    }
}

// No `## Status` section: a project's status is a field on the node
// (#143), shown as the chip in its header. New projects start "Active".
const PROJECT: &str = "\
## Goals

-

## Team

| Name | Function | Email | Comment |
|------|----------|-------|---------|
|      |          |       |         |
";

const AREA: &str = "\
## Purpose

-

## Resources

-
";

const RESOURCE: &str = "\
## Summary

-

## Links

-
";

const REFERENCE: &str = "\
## Citation

>

## Notes

-
";
