// The two pages of an item, Create item and Edit item, as the registry draws them (one form, `edit` says which).
import type { OwnProps } from "../deep/registry";
import { ItemForm } from "./item-form";

export const ItemCreate = (p: OwnProps) => <ItemForm {...p} edit={false} />;
export const ItemEdit = (p: OwnProps) => <ItemForm {...p} edit />;
