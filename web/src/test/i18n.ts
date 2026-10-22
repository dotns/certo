import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import en from '../../public/locales/en/common.json'

/** Synchronous English-only i18n for the test environment. */
i18n.use(initReactI18next).init({
  lng: 'en',
  fallbackLng: 'en',
  ns: ['common'],
  defaultNS: 'common',
  resources: { en: { common: en } },
  interpolation: { escapeValue: false },
  react: { useSuspense: false },
})

export default i18n
