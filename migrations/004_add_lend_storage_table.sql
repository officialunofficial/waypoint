-- Migration: Add lend_storage table for MESSAGE_TYPE_LEND_STORAGE
-- Description: Creates table to store storage lending messages introduced in Snapchain v0.8.1

-- Create lend_storage table
CREATE TABLE public.lend_storage
(
    id         uuid                     DEFAULT public.generate_ulid() NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    "timestamp" timestamp with time zone                                NOT NULL,
    deleted_at timestamp with time zone,
    fid        bigint                                                  NOT NULL, -- Lender FID
    to_fid     bigint                                                  NOT NULL, -- Recipient FID
    num_units  bigint                                                  NOT NULL, -- Number of storage units
    unit_type  smallint                                                NOT NULL, -- StorageUnitType: 0=legacy, 1=2024, 2=2025
    hash       bytea                                                   NOT NULL, -- Message hash
    CONSTRAINT lend_storage_pkey PRIMARY KEY (id),
    CONSTRAINT lend_storage_hash_key UNIQUE (hash)
);

-- Create indexes for efficient querying
CREATE INDEX lend_storage_fid_index ON public.lend_storage USING btree (fid);
CREATE INDEX lend_storage_to_fid_index ON public.lend_storage USING btree (to_fid);
CREATE INDEX lend_storage_timestamp_index ON public.lend_storage USING btree ("timestamp");
CREATE INDEX lend_storage_fid_to_fid_index ON public.lend_storage USING btree (fid, to_fid);
CREATE INDEX lend_storage_active_index ON public.lend_storage USING btree (fid, to_fid) WHERE (deleted_at IS NULL);

-- Add trigger for updated_at column
CREATE TRIGGER update_lend_storage_updated_at BEFORE UPDATE ON public.lend_storage
    FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();
