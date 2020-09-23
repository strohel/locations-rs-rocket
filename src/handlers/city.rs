//! Handlers for `/city/*` endpoints.

use crate::{
    response::{ErrorResponse::BadRequest, HandlerResult, JsonResult},
    services::locations_repo::{Coordinates, ElasticCity, Language, LocationsElasticRepository},
    stateful::elasticsearch::WithElastic,
    AppState,
};
use futures::{stream::FuturesOrdered, TryStreamExt};
use rocket::{
    async_trait, get,
    http::HeaderMap,
    outcome::IntoOutcome,
    request::{FormParseError, FromRequest, LenientForm, Outcome},
    FromForm, Request,
};
use rocket_contrib::json::Json;
use serde::Serialize;
use std::cmp::Reverse;
use validator::Validate;

/// Query for the `/city/v1/get` endpoint.
#[derive(FromForm)]
pub(crate) struct CityQuery {
    /// Id of the city to get, positive integer.
    id: u64,
    language: Language,
}

/// `City` API entity. All city endpoints respond with this payload (or a composition of it).
#[allow(non_snake_case)]
#[derive(Serialize)]
pub(crate) struct CityResponse {
    /// Id of the city, e.g. `123`.
    id: u64,
    /// Whether this city is marked as *featured*, e.g. `false`.
    isFeatured: bool,
    /// ISO 3166-1 alpha-2 country code, or a custom 4-letter code, e.g. `"CZ"`.
    countryIso: String,
    /// E.g. `"Plzeň"`.
    name: String,
    /// E.g. `"Plzeňský kraj"`.
    regionName: String,
}

/// Type alias to parse query parameters using a struct, catching errors, ignoring extra params.
type Parse<'f, T> = Result<LenientForm<T>, FormParseError<'f>>;

/// The `/city/v1/get` endpoint. HTTP request: [`CityQuery`], response: [`CityResponse`].
///
/// Get city of given ID localized to given language.
#[get("/city/v1/get?<query..>")]
pub(crate) async fn get(
    query: Parse<'_, CityQuery>,
    app: AppState<'_>,
) -> JsonResult<CityResponse> {
    let query = query?;
    let locations_es_repo = LocationsElasticRepository(&app);
    let es_city = locations_es_repo.get_city(query.id).await?;

    Ok(Json(es_city.into_resp(&app, query.language).await?))
}

/// Query for the `/city/v1/featured` endpoint.
#[derive(FromForm)]
pub(crate) struct FeaturedQuery {
    language: Language,
}

/// A list of `City` API entities.
#[derive(Serialize)]
pub(crate) struct MultiCityResponse {
    cities: Vec<CityResponse>,
}

/// The `/city/v1/featured` endpoint. HTTP request: [`FeaturedQuery`], response: [`MultiCityResponse`].
///
/// Returns a list of all featured cities.
#[get("/city/v1/featured?<query..>")]
pub(crate) async fn featured(
    query: Parse<'_, FeaturedQuery>,
    app: AppState<'_>,
) -> JsonResult<MultiCityResponse> {
    let query = query?;
    let locations_es_repo = LocationsElasticRepository(&app);
    let mut es_cities = locations_es_repo.get_featured_cities().await?;

    let preferred_country_iso = match query.language {
        Language::CS => "CZ",
        Language::DE => "DE",
        Language::EN => "CZ",
        Language::PL => "PL",
        Language::SK => "SK",
    };
    es_cities.sort_by_key(|c| Reverse(c.countryIso == preferred_country_iso));

    es_cities_into_resp(&app, es_cities, query.language).await
}

/// Query for the `/city/v1/search` endpoint.
#[allow(non_snake_case)]
#[derive(FromForm)]
pub(crate) struct SearchQuery {
    /// The search query.
    query: String,
    /// ISO 3166-1 alpha-2 country code. Can be used to limit scope of the search to a given country.
    countryIso: Option<String>,
    language: Language,
}

/// The `/city/v1/search` endpoint. HTTP request: [`SearchQuery`], response: [`MultiCityResponse`].
///
/// Returns list of cities matching the 'query' parameter.
/// The response is limited to 10 cities and no pagination is provided.
#[get("/city/v1/search?<query..>")]
pub(crate) async fn search(
    query: Parse<'_, SearchQuery>,
    app: AppState<'_>,
) -> JsonResult<MultiCityResponse> {
    let query = query?;
    let locations_es_repo = LocationsElasticRepository(&app);
    let es_cities =
        locations_es_repo.search(&query.query, query.language, query.countryIso.as_deref()).await?;

    es_cities_into_resp(&app, es_cities, query.language).await
}

/// Query for the `/city/v1/closest` endpoint.
#[derive(FromForm)]
pub(crate) struct ClosestQuery {
    /// Latitude in decimal degrees with . as decimal separator.
    lat: Option<f64>,
    /// Longitude in decimal degrees with . as decimal separator.
    lon: Option<f64>,
    language: Language,
}

impl ClosestQuery {
    /// Extract optional coordinates out of query, error if only one of them is given.
    fn coordinates(&self) -> HandlerResult<Option<Coordinates>> {
        match (self.lat, self.lon) {
            (Some(lat), Some(lon)) => Ok(Some(Coordinates { lat, lon })),
            (None, None) => Ok(None),
            _ => Err(BadRequest("either both or none of `lat`, `lon` expected".to_string())),
        }
    }
}

/// The `/city/v1/closest` endpoint. HTTP request: [`ClosestQuery`], response: [`CityResponse`].
///
/// Returns a single city that is closest to the coordinates.
/// If coordinates are not given we fallback to IP geo-location to find the closest featured city.
#[get("/city/v1/closest?<query..>")]
pub(crate) async fn closest(
    request_header_coords: Option<Coordinates>,
    query: Parse<'_, ClosestQuery>,
    app: AppState<'_>,
) -> JsonResult<CityResponse> {
    let query = query?;
    let locations_es_repo = LocationsElasticRepository(&app);

    let es_city = if let Some(coords) = query.coordinates()? {
        coords.validate()?; // validate explicitly, we don't want to validate when loading from ES.
        locations_es_repo.get_city_by_coords(coords, None).await?
    } else if let Some(coords) = request_header_coords {
        locations_es_repo.get_city_by_coords(coords, Some(true)).await?
    } else {
        let city_id = match query.language {
            Language::CS => 101_748_113,   // Prague
            Language::DE => 101_909_779,   // Berlin
            Language::EN => 101_748_113,   // also Prague
            Language::PL => 101_752_777,   // Warsaw
            Language::SK => 1_108_800_123, // Bratislava
        };
        locations_es_repo.get_city(city_id).await?
    };

    Ok(Json(es_city.into_resp(&app, query.language).await?))
}

/// Query for the `/city/v1/associatedFeatured` endpoint.
#[derive(FromForm)]
pub(crate) struct AssociatedFeaturedQuery {
    /// Id of the city to get associated featured city for, positive integer.
    id: u64,
    language: Language,
}

/// The `/city/v1/associatedFeatured` endpoint. HTTP request: [`AssociatedFeaturedQuery`],
/// response: [`CityResponse`].
///
/// For a given city id returns the closest featured city.
#[get("/city/v1/associatedFeatured?<query..>")]
pub(crate) async fn associated_featured(
    query: Parse<'_, AssociatedFeaturedQuery>,
    app: AppState<'_>,
) -> JsonResult<CityResponse> {
    let query = query?;
    let locations_es_repo = LocationsElasticRepository(&app);
    let mut es_city = locations_es_repo.get_city(query.id).await?;
    if !es_city.isFeatured {
        es_city = locations_es_repo.get_closest_city(es_city.centroid, Some(true)).await?;
    }

    Ok(Json(es_city.into_resp(&app, query.language).await?))
}

/// Implement Rocket request guard to parse coords from request headers. "Forwards" if not found.
#[async_trait]
impl<'a, 'r> FromRequest<'a, 'r> for Coordinates {
    type Error = ();

    async fn from_request(request: &'a Request<'r>) -> Outcome<Self, Self::Error> {
        get_request_fastly_geo_coords(request.headers()).or_forward(())
    }
}

/// Get [Coordinates] out of Fastly Geo headers or [None] if they are not set or are invalid.
fn get_request_fastly_geo_coords(headers: &HeaderMap<'_>) -> Option<Coordinates> {
    let lat = headers.get_one("Fastly-Geo-Lat")?;
    let lon = headers.get_one("Fastly-Geo-Lon")?;
    let coords = Coordinates { lat: lat.parse().ok()?, lon: lon.parse().ok()? };

    if coords.lat == 0.0 && coords.lon == 0.0 {
        return None; // Fastly returns 0, 0 in case it cannot determine IP geolocation.
    }
    Some(coords)
}

impl ElasticCity {
    /// Transform ElasticCity into CityResponse, fetching the region.
    async fn into_resp<T: WithElastic>(
        self,
        app: &T,
        language: Language,
    ) -> HandlerResult<CityResponse> {
        let locations_es_repo = LocationsElasticRepository(app);
        let es_region = locations_es_repo.get_region(self.regionId).await?;

        let name_key = language.name_key();
        let name = self.names.get(&name_key).ok_or_else(|| BadRequest(name_key.clone()))?;
        let region_name = es_region.names.get(&name_key).ok_or_else(|| BadRequest(name_key))?;

        Ok(CityResponse {
            id: self.id,
            isFeatured: self.isFeatured,
            countryIso: self.countryIso,
            name: name.to_string(),
            regionName: region_name.to_string(),
        })
    }
}

/// Convert a vector of [ElasticCity] into [MultiCityResponse], maintaining order and fetching
/// required regions asynchronously all in parallel (which is somewhat redundant with
/// [ElasticRegion] cache).
async fn es_cities_into_resp<T: WithElastic>(
    app: &T,
    es_cities: Vec<ElasticCity>,
    language: Language,
) -> JsonResult<MultiCityResponse> {
    let city_futures: FuturesOrdered<_> =
        es_cities.into_iter().map(|it| it.into_resp(app, language)).collect();

    city_futures.try_collect().await.map(|cities| Json(MultiCityResponse { cities }))
}
