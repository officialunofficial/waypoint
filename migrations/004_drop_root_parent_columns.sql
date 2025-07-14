-- Drop root parent tracking columns and indexes from casts table

-- Drop indexes first
DROP INDEX IF EXISTS idx_casts_root_parent_hash;
DROP INDEX IF EXISTS idx_casts_root_parent_url;
DROP INDEX IF EXISTS idx_casts_root_parent_fid_hash;
DROP INDEX IF EXISTS idx_casts_fid_root_parent;

-- Drop columns
ALTER TABLE public.casts
    DROP COLUMN IF EXISTS root_parent_fid,
    DROP COLUMN IF EXISTS root_parent_hash,
    DROP COLUMN IF EXISTS root_parent_url;