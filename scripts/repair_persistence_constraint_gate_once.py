#!/usr/bin/env python3
"""Apply one reviewed repair to the new Rust persistence constraint gate.

This file is deleted by the workflow that executes it. It exists only to keep the
semantic patch readable and independently fail if the expected source changed.
"""

from __future__ import annotations

from pathlib import Path

PATH = Path("tools/contract-parity/src/bin/persistence_constraint_gate.rs")


def replace_once(source: str, old: str, new: str, label: str) -> str:
    count = source.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one reviewed match, found {count}")
    return source.replace(old, new, 1)


def main() -> None:
    source = PATH.read_text(encoding="utf-8")
    source = replace_once(
        source,
        "use std::path::{Path, PathBuf};",
        "use std::path::{Component, Path, PathBuf};",
        "path import",
    )
    source = replace_once(
        source,
        """        if let (Some(minimum), Some(maximum)) = (self.min_length, self.max_length)
            && minimum > maximum
        {
            return Err(format!(
                \"{label} has minLength/minLength decorator {minimum} greater than maxLength/maxLength decorator {maximum}\"
            )
            .into());
        }
""",
        """        if let (Some(minimum), Some(maximum)) = (self.min_length, self.max_length) {
            if minimum > maximum {
                return Err(format!(
                    \"{label} has minimum length {minimum} greater than maximum length {maximum}\"
                )
                .into());
            }
        }
""",
        "MSRV-safe length validation",
    )
    source = replace_once(
        source,
        """fn resolve(root: &Path, value: PathBuf) -> Result<PathBuf> {
    let path = if value.is_absolute() {
        value
    } else {
        root.join(value)
    };
    if !path.starts_with(root) {
        return Err(format!(\"report path must remain inside {}\", root.display()).into());
    }
    Ok(path)
}
""",
        """fn resolve(root: &Path, value: PathBuf) -> Result<PathBuf> {
    let relative = if value.is_absolute() {
        value
            .strip_prefix(root)
            .map_err(|_| format!(\"report path must remain inside {}\", root.display()))?
    } else {
        value.as_path()
    };
    if relative.as_os_str().is_empty()
        || !relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            \"report path must be a normalized descendant of {}\",
            root.display()
        )
        .into());
    }
    Ok(root.join(relative))
}
""",
        "report path containment",
    )
    source = replace_once(
        source,
        """    #[test]
    fn impossible_length_range_is_rejected() {
""",
        """    #[test]
    fn report_path_traversal_is_rejected() {
        let root = tempfile::tempdir().expect(\"root\");
        assert!(resolve(root.path(), PathBuf::from(\"../outside.json\")).is_err());
        assert!(resolve(root.path(), root.path().join(\"../outside.json\")).is_err());
        assert_eq!(
            resolve(root.path(), PathBuf::from(\"target/report.json\")).unwrap(),
            root.path().join(\"target/report.json\")
        );
    }

    #[test]
    fn impossible_length_range_is_rejected() {
""",
        "path traversal regression",
    )
    PATH.write_text(source, encoding="utf-8")


if __name__ == "__main__":
    main()
