// @generated automatically by Diesel CLI.

pub mod sql_types {
    #[derive(diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "taxonomy_rank"))]
    pub struct TaxonomyRank;
}

diesel::table! {
    species_lists (id) {
        id -> Uuid,
        #[max_length = 100]
        name -> Varchar,
        description -> Nullable<Text>,
        created_at -> Timestamp,
        position -> Int4,
    }
}

diesel::table! {
    species_records (id) {
        id -> Uuid,
        list_id -> Uuid,
        taxon_id -> Uuid,
        #[max_length = 200]
        scientific_name -> Varchar,
        #[max_length = 200]
        chinese_name -> Nullable<Varchar>,
        distribution -> Nullable<Text>,
        note -> Nullable<Text>,
        source -> Nullable<Text>,
        #[max_length = 50]
        record_no -> Nullable<Varchar>,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::TaxonomyRank;

    taxa (id) {
        id -> Uuid,
        parent_id -> Nullable<Uuid>,
        rank -> TaxonomyRank,
        #[max_length = 200]
        scientific_name -> Varchar,
        #[max_length = 200]
        chinese_name -> Nullable<Varchar>,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    users (id) {
        id -> Uuid,
        #[max_length = 10]
        name -> Varchar,
        #[max_length = 50]
        username -> Varchar,
        #[max_length = 255]
        password_hash -> Varchar,
        role -> Int4,
        created_at -> Timestamp,
        token_version -> Int4,
    }
}

diesel::joinable!(species_records -> species_lists (list_id));
diesel::joinable!(species_records -> taxa (taxon_id));

diesel::allow_tables_to_appear_in_same_query!(species_lists, species_records, taxa, users,);
