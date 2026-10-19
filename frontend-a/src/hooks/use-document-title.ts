import { useEffect } from "react";

/** Sets document.title. */
export function useDocumentTitle(title: string) {
  useEffect(() => {
    if (title) document.title = title;
  }, [title]);
}
