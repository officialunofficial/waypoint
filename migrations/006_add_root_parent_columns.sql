-- Add root parent tracking columns for efficient thread identification
-- Root casts have NULL values; replies have values pointing to the thread root

ALTER TABLE public.casts ADD COLUMN root_parent_fid bigint;
ALTER TABLE public.casts ADD COLUMN root_parent_hash bytea;
ALTER TABLE public.casts ADD COLUMN root_parent_url text;

-- Index for efficient thread lookups by root hash
CREATE INDEX casts_root_parent_hash_index ON public.casts USING btree (root_parent_hash)
WHERE (root_parent_hash IS NOT NULL);

-- Index for finding threads by author (the FID who started the thread)
CREATE INDEX casts_root_parent_fid_index ON public.casts USING btree (root_parent_fid)
WHERE (root_parent_fid IS NOT NULL);

-- Index for finding casts in URL-rooted threads (e.g., channel posts)
CREATE INDEX casts_root_parent_url_index ON public.casts USING btree (root_parent_url)
WHERE (root_parent_url IS NOT NULL);
