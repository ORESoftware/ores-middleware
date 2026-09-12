package oresmiddleware

// Small, dependency-free functional helpers used to build values instead of
// mutating them in place. Go's generics are narrower than Rust's, so the
// building blocks here are deliberately tiny: a rule type, a way to run rules
// to a fresh slice, and map/filter over slices and maps that always return a
// new collection and never touch the input.
//
// Style rule for this package (see FUNCTIONAL-STYLE.md at the repo root):
// prefer `value := build(inputs)` over `var value T; fill(&value)`. Where a
// hot path deliberately mutates in place, mark it with a
// `HOT-PATH (imperative by design)` comment explaining the allocation cost.

// Rule inspects a value and reports at most one validation issue.
type Rule[T any] func(T) (ValidationIssue, bool)

// issueWhen builds a rule result: the issue is reported only when failed holds.
func issueWhen(failed bool, path, code, message string) (ValidationIssue, bool) {
	if !failed {
		return ValidationIssue{}, false
	}
	return ValidationIssue{Path: path, Code: code, Message: message}, true
}

// runRules applies every rule to value, in order, and returns a new issue slice.
// A nil result (no issues) is preserved so callers can keep `len(issues) == 0`
// and `issues == nil` checks interchangeable, as they were before.
func runRules[T any](value T, rules ...Rule[T]) ValidationIssues {
	var issues ValidationIssues
	for _, rule := range rules {
		if issue, failed := rule(value); failed {
			issues = append(issues, issue)
		}
	}
	return issues
}

// mapSlice returns a new slice holding f(item) for every item of in.
func mapSlice[In, Out any](in []In, f func(In) Out) []Out {
	if in == nil {
		return nil
	}
	out := make([]Out, 0, len(in))
	for _, item := range in {
		out = append(out, f(item))
	}
	return out
}

// filterSlice returns a new slice holding the items of in that keep accepts.
func filterSlice[T any](in []T, keep func(T) bool) []T {
	if in == nil {
		return nil
	}
	out := make([]T, 0, len(in))
	for _, item := range in {
		if keep(item) {
			out = append(out, item)
		}
	}
	return out
}

// filterMap returns a new map holding the entries of in that keep accepts.
func filterMap[K comparable, V any](in map[K]V, keep func(K, V) bool) map[K]V {
	out := make(map[K]V, len(in))
	for key, value := range in {
		if keep(key, value) {
			out[key] = value
		}
	}
	return out
}

// entry is one optional key/value pair for mapOf.
type entry[K comparable, V any] struct {
	key     K
	value   V
	present bool
}

// kv always contributes an entry.
func kv[K comparable, V any](key K, value V) entry[K, V] {
	return entry[K, V]{key: key, value: value, present: true}
}

// kvWhen contributes an entry only when present holds.
func kvWhen[K comparable, V any](present bool, key K, value V) entry[K, V] {
	return entry[K, V]{key: key, value: value, present: present}
}

// mapOf builds a fresh map from the present entries, in argument order (later
// entries win on duplicate keys, mirroring successive assignment).
func mapOf[K comparable, V any](entries ...entry[K, V]) map[K]V {
	out := make(map[K]V, len(entries))
	for _, item := range entries {
		if item.present {
			out[item.key] = item.value
		}
	}
	return out
}

// mergeMaps returns a new map with every entry of base, then every entry of
// overlay (overlay wins). Neither input is modified.
func mergeMaps[K comparable, V any](base, overlay map[K]V) map[K]V {
	out := make(map[K]V, len(base)+len(overlay))
	for key, value := range base {
		out[key] = value
	}
	for key, value := range overlay {
		out[key] = value
	}
	return out
}
