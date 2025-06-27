-- Add root parent tracking columns to casts table
ALTER TABLE public.casts
    ADD COLUMN root_parent_fid bigint,
    ADD COLUMN root_parent_hash bytea,
    ADD COLUMN root_parent_url text;

-- Add indexes for efficient thread/conversation queries
CREATE INDEX idx_casts_root_parent_hash ON public.casts USING btree (root_parent_hash) 
    WHERE (root_parent_hash IS NOT NULL);

CREATE INDEX idx_casts_root_parent_url ON public.casts USING btree (root_parent_url) 
    WHERE (root_parent_url IS NOT NULL);

-- Composite index for querying all casts in a thread by root parent
CREATE INDEX idx_casts_root_parent_fid_hash ON public.casts USING btree (root_parent_fid, root_parent_hash) 
    WHERE (root_parent_hash IS NOT NULL);

-- Index for finding threads by fid (useful for muting threads by a specific user)
CREATE INDEX idx_casts_fid_root_parent ON public.casts USING btree (fid, root_parent_hash)
    WHERE (root_parent_hash IS NOT NULL AND deleted_at IS NULL);

-- Comment on new columns
COMMENT ON COLUMN public.casts.root_parent_fid IS 'FID of the root parent cast (top of the thread)';
COMMENT ON COLUMN public.casts.root_parent_hash IS 'Hash of the root parent cast (top of the thread)';
COMMENT ON COLUMN public.casts.root_parent_url IS 'URL of the root parent if thread started with URL parent';