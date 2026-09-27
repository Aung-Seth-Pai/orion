import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// Without this, a component rendered in one test is still in the document for
// the next one, and queries start matching the wrong element.
afterEach(cleanup);
