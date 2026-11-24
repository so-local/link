---
name: java
description: Build, test and write idiomatic Java or Kotlin with Maven or Gradle, using the project's wrapper.
---

# Java and Kotlin

## Detect

`pom.xml` is Maven; `build.gradle(.kts)` and `settings.gradle(.kts)` is
Gradle (Kotlin if `kotlin("jvm")` or `*.kt` files). Prefer the wrapper in
the repository, which pins the tool version, run by `sh` (`sh ./mvnw`,
`sh ./gradlew`): your sandbox runs no program from a folder it writes. The
JDK version is in `pom.xml` (`maven.compiler.release`), Gradle
`toolchain`, or `.java-version`/`.sdkmanrc`. Check `java -version`.
Multi-module: `<modules>` or `include(...)`.

## Commands

Maven:
```sh
sh ./mvnw -q verify                       # compile, test, package, checks
sh ./mvnw test -Dtest=ClassTest#method    # one test
sh ./mvnw -pl module -am test             # one module and what it needs
sh ./mvnw spotless:check checkstyle:check # if configured
```
Gradle:
```sh
sh ./gradlew build                        # compile, test, checks
sh ./gradlew test --tests 'pkg.ClassTest.method'
sh ./gradlew :module:test
sh ./gradlew spotlessCheck ktlintCheck detekt   # whichever the project has
```
Add `--offline` only if dependencies are cached; otherwise the network
resolves them. Plain `javac`/`kotlinc` only for single files.

## Idioms

- Java: immutable data (`record`, `final`), `Optional` for absent return
  values (not fields or parameters), try-with-resources, streams where
  they read clearly, `equals`/`hashCode` together, `List.of`.
- Specific exceptions, never swallowed; do not catch `Throwable`.
- Kotlin: `val` over `var`, null safety without `!!`, data classes,
  sealed classes with `when`, coroutines with structured scope.
- Tests: JUnit 5 (`@Test`, `@ParameterizedTest`), AssertJ or the existing
  assertion library, Mockito sparingly.
- Follow the existing formatter; do not reorganize imports by hand.

## Pitfalls

Mutable shared state and thread safety; `==` on strings; time zones in
`java.time`; checked exceptions; Spring or framework magic (annotations
change behavior: read config before assuming). Do not edit generated
sources or `target/`, `build/`. Dependency versions belong in the
BOM or version catalog.
