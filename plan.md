1. **Analyze the Issue**:
   The CI failure happens in the `test (macos)` job for `test/renderer/command-bindings-runtime.test.js` because of these two assertions:
   ```
   AssertionError [ERR_ASSERTION]: Expected values to be strictly equal:
   + actual - expected
   + 'Save (⌘s)'
   - 'Save (Ctrl+s)'
   ```
   ```
   AssertionError [ERR_ASSERTION]: Expected values to be strictly equal:
   + actual - expected
   + 'Increase size (⌘+)'
   - 'Increase size (Ctrl++)'
   ```
   This means that when the tests run on macOS, `navigator.userAgentData?.platform || navigator.platform` picks up macOS and successfully translates `Mod+` into `⌘`, but the test assertions statically expected `'Save (Ctrl+s)'` and `'Increase size (Ctrl++)'`.

2. **Fix the Tests in `test/renderer/command-bindings-runtime.test.js`**:
   The test should dynamically calculate the expected string based on the platform, just like `bindControls` does. Or, since this is a unit test, we can mock the `navigator` object to always return a specific platform (e.g. `linux` or `win32`) so the test is deterministic regardless of where it runs.

   In `test/renderer/command-bindings-runtime.test.js`:
   We can mock the `navigator` object for the execution of `CB.bindControls`. But wait, in Node.js `navigator` might not exist unless it's globally mocked. Wait, if the test is running in Node.js (`test_runner`), `navigator` is globally available since Node 21.
   Let's check if we can simply override `globalThis.navigator = { platform: 'linux' }` or similar before the tests, or just within those specific tests.
   Wait, if `navigator` is already defined as a getter in Node.js, we might need to `Object.defineProperty(globalThis, 'navigator', ...)`.

   Let's write a small script to test modifying `globalThis.navigator`.
