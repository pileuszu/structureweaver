-- StructureWeaver inferred SQLite schema draft. Review before applying.
-- PKs are inferred from observed unique identifiers; FK candidates are comments only.

-- Source table: sheet-1_t1 (3 observed records)
CREATE TABLE "Customers" (
    "customer_id" INT NOT NULL PRIMARY KEY,
    "customer_name" TEXT NOT NULL,
    "country" TEXT NOT NULL
);

-- Source table: sheet-2_t2 (4 observed records)
CREATE TABLE "Orders" (
    "order_id" INT NOT NULL PRIMARY KEY,
    "customer_id" INTEGER NOT NULL,
    "amount" INTEGER NOT NULL
);
-- Candidate only: FOREIGN KEY ("customer_id") REFERENCES "Customers" ("customer_id");
-- Evidence: Compatible INTEGER types and ID naming; all 4 observed non-null values (3 distinct) reference the unique Customers.customer_id key. Source values repeat. Source key has no nulls. Observed data only; requires review.

-- Source table: sheet-3_t3 (5 observed records)
CREATE TABLE "LineItems" (
    "line_id" INT NOT NULL PRIMARY KEY,
    "order_id" INTEGER NOT NULL,
    "product_name" TEXT NOT NULL,
    "quantity" INTEGER NOT NULL
);
-- Candidate only: FOREIGN KEY ("order_id") REFERENCES "Orders" ("order_id");
-- Evidence: Compatible INTEGER types and ID naming; all 5 observed non-null values (4 distinct) reference the unique Orders.order_id key. Source values repeat. Source key has no nulls. Observed data only; requires review.

