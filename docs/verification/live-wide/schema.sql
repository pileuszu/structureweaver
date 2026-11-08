-- StructureWeaver inferred SQLite schema draft. Review before applying.
-- PKs are inferred from observed unique identifiers; FK candidates are comments only.

-- Source table: sheet-1_table_1 (3 observed records)
CREATE TABLE "Wide Customers" (
    "customer_id" INTEGER NOT NULL,
    "customer_name" TEXT NOT NULL,
    "country" TEXT NOT NULL,
    "city" TEXT NOT NULL,
    "segment" TEXT NOT NULL,
    "active" BOOLEAN NOT NULL,
    "orders_count" INTEGER NOT NULL,
    "revenue" INTEGER NOT NULL,
    "discount" REAL NOT NULL,
    "currency" TEXT NOT NULL,
    "account_owner" TEXT NOT NULL,
    "postal_code" TEXT NOT NULL
);

