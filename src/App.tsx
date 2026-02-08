import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import { AppShell } from './components/layout/AppShell';
import { PlaceholderPage } from './pages/placeholder';
import { TasksPage } from './pages/tasks';

export default function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route element={<AppShell />}>
          <Route path="/" element={<Navigate to="/tasks" replace />} />
          <Route path="/tasks" element={<TasksPage />} />
          <Route path="/analytics" element={<PlaceholderPage title="Analytics" />} />
          <Route path="/review" element={<PlaceholderPage title="Review" />} />
          <Route path="/integrations/github" element={<PlaceholderPage title="GitHub" />} />
          <Route path="/integrations/linear" element={<PlaceholderPage title="Linear" />} />
          <Route path="/integrations/sentry" element={<PlaceholderPage title="Sentry" />} />
          <Route path="/notes" element={<PlaceholderPage title="Notes" />} />
          <Route path="/skills" element={<PlaceholderPage title="Skills" />} />
          <Route path="/terminal" element={<PlaceholderPage title="Terminal" />} />
          <Route path="/accounts" element={<PlaceholderPage title="Accounts" />} />
          <Route path="/settings/*" element={<PlaceholderPage title="Settings" />} />
        </Route>
      </Routes>
    </BrowserRouter>
  );
}
