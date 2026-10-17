import { useState, useEffect } from 'react';
import type { FormEvent } from 'react';
import { useNavigate, Link } from 'react-router-dom';
import { api } from '../api';
import { setToken } from '../auth';

export default function Register() {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const navigate = useNavigate();

  useEffect(() => { document.title = 'Create Account — certo'; }, []);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setError('');
    if (password.length < 6) {
      setError('Password must be at least 6 characters');
      return;
    }
    setLoading(true);
    try {
      const res = await api.register(username, password);
      setToken(res.token);
      navigate('/dashboard');
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Registration failed');
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="auth-page">
      <div className="auth-card">
        <div className="auth-logo">CERTO</div>
        <div className="auth-tagline">ACME httpreq Server</div>
        {error && <div className="error">{error}</div>}
        <form onSubmit={handleSubmit}>
          <input type="text" placeholder="Choose a username" value={username}
            onChange={(e) => setUsername(e.target.value)} required autoFocus />
          <input type="password" placeholder="Create a password (min 6 chars)" value={password}
            onChange={(e) => setPassword(e.target.value)} required minLength={6} />
          <button type="submit" className="btn-primary" disabled={loading}>
            {loading ? 'Creating account...' : 'Create account'}
          </button>
        </form>
        <p className="auth-link">
          Already have an account? <Link to="/login">Sign in</Link>
        </p>
        <div className="auth-footer">
          <a href="https://github.com/dotns/certo" target="_blank" rel="noreferrer">GitHub</a>
          <span>&middot;</span>
          <a href="/llms.txt" target="_blank">llms.txt</a>
        </div>
      </div>
    </div>
  );
}
