import type { RecipeCost } from "../types/gameData/recipes";

/**
 * Display names for recipe `Costs` currencies. The CDN stores them as
 * PascalCase identifiers ("CombatWisdom"); anything not listed here falls
 * back to splitting the identifier on word boundaries.
 */
const CURRENCY_LABELS: Record<string, string> = {
  CombatWisdom: "Combat Wisdom",
  FaeEnergy: "Fae Energy",
  GuildCredits: "Guild Credits",
  GlamourCredits: "Glamour Credits",
};

export function currencyLabel(currency: string): string {
  return CURRENCY_LABELS[currency] ?? currency.replace(/([a-z0-9])([A-Z])/g, "$1 $2");
}

/** Format a single currency cost, e.g. "650 Combat Wisdom". */
export function formatRecipeCost(cost: RecipeCost): string {
  return `${cost.price.toLocaleString()} ${currencyLabel(cost.currency)}`;
}

/** Format a whole cost list, e.g. "650 Combat Wisdom · 2 Guild Credits". */
export function formatRecipeCosts(costs: RecipeCost[] | undefined | null): string {
  if (!costs?.length) return "";
  return costs.map(formatRecipeCost).join(" · ");
}
