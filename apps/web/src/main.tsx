import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import './store/theme';
import './store/terminalTheme';
import './index.css';

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
