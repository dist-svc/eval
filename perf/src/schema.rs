
diesel::table! {
    tests (id) {
        id -> Integer,
        at -> Timestamp,
        notes -> Text,
    }
}

diesel::table! {
    configs (id) {
        id -> Integer,
        num_subscribers -> Integer,
        num_publishers -> Integer,
        message_size -> Integer,
        publish_period -> Varchar,
        disabled -> Integer,
    }
}

diesel::table! {
    results (id) {
        id -> Integer,
        mode -> Varchar,
        title -> Varchar,
        packet_loss -> Double,
        throughput -> Double,
    }
}
