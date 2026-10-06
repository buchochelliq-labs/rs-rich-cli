-- A small shop: customers place orders of products.
CREATE TABLE customers (
    id      BIGINT PRIMARY KEY,
    email   VARCHAR(255) NOT NULL UNIQUE,
    name    TEXT
);

CREATE TABLE products (
    sku     VARCHAR(32) PRIMARY KEY,
    title   TEXT NOT NULL,
    price   NUMERIC(10, 2) NOT NULL CHECK (price >= 0)
);

CREATE TABLE orders (
    id          BIGINT PRIMARY KEY,
    customer_id BIGINT NOT NULL REFERENCES customers (id),
    status      VARCHAR(16) DEFAULT 'new',
    placed_at   TIMESTAMP WITH TIME ZONE NOT NULL
);

CREATE TABLE order_lines (
    order_id    BIGINT NOT NULL REFERENCES orders (id),
    sku         VARCHAR(32) NOT NULL REFERENCES products (sku),
    quantity    INT NOT NULL DEFAULT 1,
    PRIMARY KEY (order_id, sku)
);

CREATE INDEX orders_customer ON orders (customer_id);
