use actix_web::{Responder, web};

#[actix_web::get("/collections")]
async fn get_collections() -> impl Responder {
    let collections = vec!["collection1", "collection2", "collection3"];
    actix_web::HttpResponse::Ok().json(collections)
}

#[actix_web::get("/collections/{collection_name}")]
async fn get_collection(collection_name: web::Path<String>) -> impl Responder {
    let collection = format!("Details of collection: {}", collection_name.into_inner());
    actix_web::HttpResponse::Ok().body(collection)
}
