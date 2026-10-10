// @generated automatically by Diesel CLI.

pub mod sql_types {
    #[derive(diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "taxonomy_rank"))]
    pub struct TaxonomyRank;
}

diesel::table! {
    bird_records (id) {
        id -> Uuid,
        taxon_id -> Uuid,
        #[max_length = 200]
        scientific_name -> Varchar,
        #[max_length = 200]
        chinese_name -> Nullable<Varchar>,
        #[max_length = 200]
        observer -> Nullable<Varchar>,
        #[max_length = 100]
        observed_at -> Nullable<Varchar>,
        #[max_length = 200]
        location -> Nullable<Varchar>,
        note -> Nullable<Text>,
        source -> Nullable<Text>,
        created_at -> Timestamp,
        updated_at -> Timestamp,
        observed_from -> Nullable<Date>,
        observed_to -> Nullable<Date>,
    }
}

diesel::table! {
    maintenance_logs (id) {
        id -> Uuid,
        list_id -> Nullable<Uuid>,
        #[max_length = 50]
        entry_date -> Nullable<Varchar>,
        #[max_length = 200]
        author -> Nullable<Varchar>,
        summary -> Text,
        species_appendix -> Nullable<Text>,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
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
    species_photos (id) {
        id -> Uuid,
        taxon_id -> Uuid,
        #[max_length = 200]
        photographer -> Nullable<Varchar>,
        #[max_length = 50]
        uploader_username -> Varchar,
        taken_at -> Date,
        #[max_length = 200]
        location -> Nullable<Varchar>,
        note -> Nullable<Text>,
        rating -> Nullable<Int2>,
        is_important -> Bool,
        file_path -> Text,
        #[max_length = 255]
        original_filename -> Nullable<Varchar>,
        file_size -> Int8,
        #[max_length = 100]
        content_type -> Nullable<Varchar>,
        created_at -> Timestamp,
        updated_at -> Timestamp,
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

diesel::joinable!(bird_records -> taxa (taxon_id));
diesel::joinable!(maintenance_logs -> species_lists (list_id));
diesel::joinable!(species_photos -> taxa (taxon_id));
diesel::joinable!(species_records -> species_lists (list_id));
diesel::joinable!(species_records -> taxa (taxon_id));

diesel::allow_tables_to_appear_in_same_query!(
    bird_records,
    maintenance_logs,
    species_lists,
    species_photos,
    species_records,
    taxa,
    users,
);
