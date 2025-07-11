#!/bin/bash
set -e

# Default values
DB_HOST="${DB_HOST:-localhost}"
DB_PORT="${DB_PORT:-5432}"
DB_USER="${DB_USER:-postgres}"
DB_NAME="${DB_NAME:-waypoint}"
MIGRATIONS_DIR="${MIGRATIONS_DIR:-migrations}"

echo "Running database migrations..."
echo "Database: $DB_USER@$DB_HOST:$DB_PORT/$DB_NAME"

# Find all migration files and sort them
for migration in $(ls $MIGRATIONS_DIR/*.sql | sort); do
    echo "Applying migration: $migration"
    psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d "$DB_NAME" -f "$migration"
done

echo "All migrations completed successfully!"