import { toast } from "sonner";

/** Copies text and says so in a toast; a browser that refuses (no permission, an insecure origin) says that instead. */
export async function copyText(text: string, what = "Copied") {
  try {
    await navigator.clipboard.writeText(text);
    toast.success(what, { description: text.length > 80 ? `${text.slice(0, 77)}…` : text, duration: 1800 });
    return true;
  } catch {
    toast.error("The browser did not allow copying", { description: "Select the text and copy it by hand." });
    return false;
  }
}
