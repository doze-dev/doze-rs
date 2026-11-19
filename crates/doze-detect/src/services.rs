//! Services doze knows how to run but has no module for (core plan O9: mailpit, minio, nats and friends stay `service`
//! recipes here; real modules for them are a later plan). Each is how to run it from a binary on the PATH and as a
//! container, and, for mongo, how a project shows it uses one (Go's `knownServices` and `serviceKinds`).

/// One service recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recipe {
    pub name: &'static str,
    /// The program it runs as; empty when only a container is known.
    pub binary: &'static str,
    /// The `run` line for that program, with `$HOST`, `$PORT`, `$DATA`.
    pub run: &'static str,
    pub port: u16,
    /// The image for the container form.
    pub image: &'static str,
    /// The port inside the container.
    pub container_port: u16,
    /// Go import-path prefixes, npm packages, compose images and variables that mean a project uses it.
    pub go: &'static [&'static str],
    pub npm: &'static [&'static str],
    pub images: &'static [&'static str],
    pub envs: &'static [&'static str],
    /// What an app's variable gets in front of the address (`mongodb://`): its variables name URLs.
    pub url_scheme: &'static str,
}

const NONE: Recipe = Recipe {
    name: "",
    binary: "",
    run: "",
    port: 0,
    image: "",
    container_port: 0,
    go: &[],
    npm: &[],
    images: &[],
    envs: &[],
    url_scheme: "",
};

/// Every recipe, by name.
pub const RECIPES: &[Recipe] = &[
    Recipe {
        name: "clickhouse",
        port: 8123,
        image: "clickhouse/clickhouse-server",
        container_port: 8123,
        ..NONE
    },
    Recipe {
        name: "mailpit",
        binary: "mailpit",
        run: "mailpit --smtp $HOST:$PORT --listen $HOST:$((PORT + 1)) --database $DATA/mail.db",
        port: 1025,
        image: "axllent/mailpit",
        container_port: 1025,
        ..NONE
    },
    Recipe {
        name: "meilisearch",
        binary: "meilisearch",
        run: "meilisearch --http-addr $HOST:$PORT --db-path $DATA",
        port: 7700,
        image: "getmeili/meilisearch",
        container_port: 7700,
        ..NONE
    },
    Recipe {
        name: "memcached",
        binary: "memcached",
        run: "memcached -l $HOST -p $PORT",
        port: 11211,
        image: "memcached",
        container_port: 11211,
        ..NONE
    },
    Recipe {
        name: "minio",
        binary: "minio",
        run: "minio server $DATA --address $HOST:$PORT --console-address $HOST:$((PORT + 1))",
        port: 9000,
        image: "minio/minio",
        container_port: 9000,
        ..NONE
    },
    Recipe {
        name: "mongo",
        binary: "mongod",
        run: "mongod --dbpath $DATA --bind_ip $HOST --port $PORT",
        port: 27017,
        image: "mongo:7",
        container_port: 27017,
        go: &["go.mongodb.org/mongo-driver"],
        npm: &["mongodb", "mongoose"],
        images: &["mongo"],
        envs: &["MONGO_URL", "MONGODB_URI", "MONGO_URI"],
        url_scheme: "mongodb://",
    },
    Recipe {
        name: "nats",
        binary: "nats-server",
        run: "nats-server -a $HOST -p $PORT -sd $DATA",
        port: 4222,
        image: "nats",
        container_port: 4222,
        ..NONE
    },
    Recipe {
        name: "opensearch",
        port: 9200,
        image: "opensearchproject/opensearch",
        container_port: 9200,
        ..NONE
    },
    Recipe {
        name: "rabbitmq",
        port: 5672,
        image: "rabbitmq:3",
        container_port: 5672,
        ..NONE
    },
];

/// The recipe called `name`.
pub fn recipe(name: &str) -> Option<&'static Recipe> {
    RECIPES.iter().find(|r| r.name == name)
}

/// The program a service is run from: its recipe's, else its own name.
pub fn binary(name: &str) -> &str {
    recipe(name)
        .map(|r| r.binary)
        .filter(|b| !b.is_empty())
        .unwrap_or(name)
}

/// The image for `--as container`: the recipe's, else the name itself.
pub fn image(name: &str) -> &str {
    recipe(name)
        .map(|r| r.image)
        .filter(|i| !i.is_empty())
        .unwrap_or(name)
}

/// Every recipe's name, for "did you mean".
pub fn names() -> Vec<String> {
    RECIPES.iter().map(|r| r.name.to_owned()).collect()
}
