---
name: c-cpp
description: Build, test and write safe C or C++ with CMake or Make, with warnings and sanitizers on.
---

# C and C++

## Detect

`CMakeLists.txt` (CMake, often with `CMakePresets.json`), `Makefile`,
`meson.build`, or `configure.ac`/`configure`. `compile_commands.json`
feeds clangd and clang-tidy. `.clang-format` and `.clang-tidy` hold style.
The standard (`-std=c11`, `CMAKE_CXX_STANDARD 17/20`) is in the build
files. Check `gcc --version`/`clang --version`, `cmake --version`.

## Commands

Build outside the source tree:
```sh
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Debug \
  -DCMAKE_EXPORT_COMPILE_COMMANDS=ON       # or: cmake --preset <name>
cmake --build build -j
ctest --test-dir build --output-on-failure   # one: -R name
```
Make: `make -j`, `make test` or `make check`. Single file:
`g++ -std=c++20 -Wall -Wextra -g file.cpp -o out`.

Sanitizers for bugs: add `-fsanitize=address,undefined -g -O1` (and
`-fsanitize=thread` separately) to a debug build, or run `valgrind`.
Format with `clang-format -i <changed files>` only if `.clang-format`
exists; lint with `clang-tidy -p build file.cpp`.

## Idioms

- Warnings on (`-Wall -Wextra`) and fixed, not silenced.
- C++: RAII, `std::unique_ptr`/`make_unique`, `std::string`,
  `std::vector`, `std::string_view`/`std::span` for views, `const` and
  `override`, no raw `new`/`delete`, no C-style casts. Rule of zero or
  five.
- C: check every return value and `malloc`; pair each allocation with one
  free; `snprintf` over `sprintf`; `size_t` for sizes; header guards.
- Match the project's standard and error style (exceptions or codes).

## Pitfalls

Undefined behavior: out-of-bounds, use after free or dangling views,
signed overflow, uninitialized variables, data races, iterator
invalidation, object lifetime in lambdas. Integer conversions and buffer
sizes are the usual security bugs (security-check skill). Do not commit
`build/` or objects. Headers: include what you use; avoid `using namespace`
in headers. ABI and public headers are interfaces: ask before changing.
