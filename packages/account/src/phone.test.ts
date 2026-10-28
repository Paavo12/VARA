import { describe, expect, it } from "vitest";
import { adultMaxDate, normalisePhone } from "./phone";

describe("normalisePhone", () => {
  it("accepts the usual Namibian formats", () => {
    for (const raw of [
      "081 123 4567",
      "+264 81 123 4567",
      "00264811234567",
      "(081) 123-4567",
      "811234567",
    ]) {
      expect(normalisePhone(raw)).toBe("+264811234567");
    }
  });

  it("rejects foreign and malformed numbers", () => {
    for (const raw of ["+27821234567", "0812", "+2640812345678", "abc", ""]) {
      expect(normalisePhone(raw)).toBeNull();
    }
  });
});

describe("adultMaxDate", () => {
  it("is 18 years before today", () => {
    expect(adultMaxDate(new Date(2026, 9, 27))).toBe("2008-10-27");
  });
});
