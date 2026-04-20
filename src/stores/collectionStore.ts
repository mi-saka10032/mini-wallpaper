import { create } from "zustand";
import type { Collection } from "@/api/config";
import {
  getAll as fetchAllCollections,
  create as createCollectionApi,
  rename as renameCollectionApi,
  remove as removeCollectionApi,
} from "@/api/collection";

export type { Collection } from "@/api/config";

interface CollectionState {
  collections: Collection[];
  fetchCollections: () => Promise<void>;
  createCollection: (name: string) => Promise<void>;
  renameCollection: (id: number, name: string) => Promise<void>;
  deleteCollection: (id: number) => Promise<void>;
}

export const useCollectionStore = create<CollectionState>((set, get) => ({
  collections: [],

  fetchCollections: async () => {
    try {
      const list = await fetchAllCollections();
      set({ collections: list });
    } catch (e) {
      console.error("[fetchCollections]", e);
    }
  },

  createCollection: async (name: string) => {
    try {
      await createCollectionApi(name);
      await get().fetchCollections();
    } catch (e) {
      console.error("[createCollection]", e);
    }
  },

  renameCollection: async (id: number, name: string) => {
    try {
      await renameCollectionApi(id, name);
      await get().fetchCollections();
    } catch (e) {
      console.error("[renameCollection]", e);
    }
  },

  deleteCollection: async (id: number) => {
    try {
      await removeCollectionApi(id);
      await get().fetchCollections();
    } catch (e) {
      console.error("[deleteCollection]", e);
    }
  },
}));
