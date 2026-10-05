import { expect, test } from '@playwright/test';

test.describe('I18n', () => {
  test.describe('Language Switching', () => {
    test('should switch language from English to French using dropdown and verify text on the homepage', async ({ page }) => {
      await page.goto('/');

      await expect(page.getByText('Know the')).toBeVisible();

      await page.getByRole('button', { name: 'Change language' }).click();
      await page.getByText('Français').click();

      await expect(page.getByText('Connaissez le')).toBeVisible();
    });

    test('should switch language from English to French using URL and verify text on the homepage', async ({ page }) => {
      await page.goto('/');

      await expect(page.getByText('Know the')).toBeVisible();

      await page.goto('/fr');

      await expect(page.getByText('Connaissez le')).toBeVisible();
    });
  });
});
