-- the shop's first migration
create table if not exists orders (id serial primary key, sku text not null);
