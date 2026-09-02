// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

fn main() {
    // `option_env!("SILVA_VIZ_BUILD_ID")` is baked in when the crate compiles,
    // and cargo has no way to know the value changed — so without this, a web
    // rebuild would happily reuse a crate compiled under a different id and
    // report the wrong one. Which is precisely the stale-bundle confusion the
    // id exists to prevent.
    println!("cargo:rerun-if-env-changed=SILVA_VIZ_BUILD_ID");
}
