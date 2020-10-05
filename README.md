# locations-rs-rocket (Locations Service in Rust & Rocket)

Small but production-ready webservice in Rust, using [Rocket](https://rocket.rs/) web framework.

The service implements [an API specification](https://github.com/strohel/locations-rs/blob/master/api-spec.md) of one feature for
[goout.net platform](https://goout.net/).
It is a port of [locations-rs](https://github.com/strohel/locations-rs) from Actix to Rocket.

## Alternatives

Multiple implementations of this service exist in different frameworks, languages for comparison.

- [locations-kt-http4k](https://gitlab.com/gooutopensource/locations-kt-http4k) in Kotlin http4k; complete,
- [locations-kt-ktor](https://gitlab.com/gooutopensource/locations-kt-ktor) in Kotlin Ktor; less complete,
- [locations-rs](https://github.com/strohel/locations-rs) in Rust Actix, complete.

## Build, Build Documentation, Run

`locations-rs-rocket` is a standard Rust binary crate.
[Same instructions as in locations-rs](https://github.com/strohel/locations-rs#build-build-documentation-run) apply.

## Runtime Dependencies

The locations service needs an Elasticsearch instance to operate.
Use [resources and recipes from locations-rs repository](https://github.com/strohel/locations-rs#runtime-dependencies).

## License

This project is licensed under [GNU Affero General Public License, version 3](https://www.gnu.org/licenses/agpl-3.0.html).
