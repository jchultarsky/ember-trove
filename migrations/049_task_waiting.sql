-- S5a (#314, docs/ONTOLOGY.md §9.3): Waiting is a stored task state. The
-- task is on someone or something else.
--
-- Its own migration: a value added to an enum can't be used in the
-- transaction that adds it, and each migration is one transaction. 050 uses
-- nothing of it either; the API is the first to write it.
ALTER TYPE task_status ADD VALUE IF NOT EXISTS 'waiting' AFTER 'open';
