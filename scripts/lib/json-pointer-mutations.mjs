function decodePointerSegment(segment) {
  return segment.replaceAll("~1", "/").replaceAll("~0", "~");
}

function parsePointer(pointer) {
  if (pointer === "") return [];
  if (typeof pointer !== "string" || !pointer.startsWith("/")) {
    throw new Error(`mutation path must be a JSON Pointer: ${String(pointer)}`);
  }
  return pointer.slice(1).split("/").map(decodePointerSegment);
}

function arrayIndex(segment, length, {allowAppend = false} = {}) {
  if (allowAppend && segment === "-") return length;
  if (!/^(0|[1-9][0-9]*)$/.test(segment)) {
    throw new Error(`array mutation index is invalid: ${segment}`);
  }
  const index = Number(segment);
  if (!Number.isSafeInteger(index) || index < 0 || index >= length + Number(allowAppend)) {
    throw new Error(`array mutation index is out of range: ${segment}`);
  }
  return index;
}

function resolve(root, segments) {
  let current = root;
  for (const segment of segments) {
    if (Array.isArray(current)) {
      current = current[arrayIndex(segment, current.length)];
    } else if (current && typeof current === "object" && Object.hasOwn(current, segment)) {
      current = current[segment];
    } else {
      throw new Error(`mutation path does not exist: /${segments.join("/")}`);
    }
  }
  return current;
}

function resolveParent(root, segments) {
  if (segments.length === 0) {
    throw new Error("root replacement is not supported by this mutation helper");
  }
  return {
    parent: resolve(root, segments.slice(0, -1)),
    key: segments.at(-1),
  };
}

export function applyJsonPointerMutations(value, mutations) {
  const clone = structuredClone(value);
  for (const [mutationIndex, mutation] of mutations.entries()) {
    if (!mutation || typeof mutation !== "object" || Array.isArray(mutation)) {
      throw new Error(`mutation ${mutationIndex} must be an object`);
    }
    const segments = parsePointer(mutation.path);

    if (mutation.op === "swap") {
      const target = resolve(clone, segments);
      if (!Array.isArray(target)) {
        throw new Error(`swap target is not an array: ${mutation.path}`);
      }
      if (!Array.isArray(mutation.value) || mutation.value.length !== 2) {
        throw new Error(`swap mutation requires exactly two indexes: ${mutation.path}`);
      }
      const left = arrayIndex(String(mutation.value[0]), target.length);
      const right = arrayIndex(String(mutation.value[1]), target.length);
      [target[left], target[right]] = [target[right], target[left]];
      continue;
    }

    const {parent, key} = resolveParent(clone, segments);
    if (mutation.op === "set") {
      if (Array.isArray(parent)) {
        const index = arrayIndex(key, parent.length, {allowAppend: true});
        parent[index] = structuredClone(mutation.value);
      } else if (parent && typeof parent === "object") {
        parent[key] = structuredClone(mutation.value);
      } else {
        throw new Error(`set parent is not a container: ${mutation.path}`);
      }
      continue;
    }

    if (mutation.op === "delete") {
      if (Array.isArray(parent)) {
        parent.splice(arrayIndex(key, parent.length), 1);
      } else if (parent && typeof parent === "object" && Object.hasOwn(parent, key)) {
        delete parent[key];
      } else {
        throw new Error(`delete target does not exist: ${mutation.path}`);
      }
      continue;
    }

    throw new Error(`unsupported mutation operation: ${String(mutation.op)}`);
  }
  return clone;
}
