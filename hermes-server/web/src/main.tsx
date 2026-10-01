// The composition root: the only place that picks the concrete implementations (HTTP client, the store) and hands them to the app.
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './app/App';
import { AUTH_REQUIRED } from './app/useBoot';
import { createHttpHubClient } from './hub/HttpHubClient';
import { HubProvider } from './state/context';
import { HubStore } from './state/HubStore';
import './styles/app.css';

const client = createHttpHubClient({ onAuthRequired: () => window.dispatchEvent(new Event(AUTH_REQUIRED)) });
const store = new HubStore(client);

// `#app` is in index.html, always present: the `!` is safe.
createRoot(document.getElementById('app')!).render(
  <StrictMode>
    <HubProvider client={client} store={store}>
      <App />
    </HubProvider>
  </StrictMode>,
);
